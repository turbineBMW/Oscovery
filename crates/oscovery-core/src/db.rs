//! The local catalog: every entry of the index in SQLite, with an FTS5 table
//! over the words people search by. One connection, on the main thread;
//! queries over a few thousand rows take a millisecond. Migrations are
//! append-only, tracked by `PRAGMA user_version`.

use crate::error::Result;
use crate::models::{Entry, Kind};
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

const MIGRATIONS: &[&str] = &[
    // 1: entries as JSON plus the columns we filter and sort by; the FTS
    // mirror; a key-value table for the fetch state (ETag, URL, when).
    "CREATE TABLE entries (
        id TEXT PRIMARY KEY,
        kind TEXT NOT NULL,
        name TEXT NOT NULL,
        author_key TEXT NOT NULL DEFAULT '',
        category TEXT NOT NULL DEFAULT '',
        stars INTEGER NOT NULL DEFAULT 0,
        first_seen TEXT NOT NULL,
        updated_at TEXT,
        json TEXT NOT NULL
    );
    CREATE INDEX entries_author ON entries(author_key);
    CREATE INDEX entries_first_seen ON entries(first_seen);
    CREATE INDEX entries_category ON entries(kind, category);
    CREATE VIRTUAL TABLE entries_fts USING fts5(
        id UNINDEXED, name, summary, description, tags, author, category,
        tokenize = 'porter unicode61'
    );
    CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
];

pub struct Database {
    conn: Connection,
}

/// What Browse asks for.
#[derive(Clone, Debug, Default)]
pub struct Query {
    /// An FTS5 `MATCH` expression (see `retrieve::fts_match`), or none for
    /// everything.
    pub fts: Option<String>,
    pub kind: Option<Kind>,
    pub category: Option<String>,
    pub sort: Sort,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// FTS rank when there is a text, else stars.
    #[default]
    Relevance,
    Stars,
    Newest,
    Updated,
    Name,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        for (index, migration) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            self.conn.execute_batch(migration)?;
            self.conn
                .pragma_update(None, "user_version", index as i64 + 1)?;
        }
        Ok(())
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    /// Replace the whole catalog with this index's entries, in one
    /// transaction so a reader never sees it half done.
    pub fn replace_entries(&mut self, entries: &[Entry]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("DELETE FROM entries; DELETE FROM entries_fts;")?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO entries (id, kind, name, author_key, category, stars, first_seen, updated_at, json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            let mut insert_fts = tx.prepare(
                "INSERT INTO entries_fts (id, name, summary, description, tags, author, category)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for entry in entries {
                insert.execute(params![
                    entry.id,
                    entry.kind.id(),
                    entry.name,
                    entry.author_key,
                    entry.category,
                    entry.stars,
                    entry.first_seen.to_string(),
                    entry.updated_at.map(|t| t.to_rfc3339()),
                    serde_json::to_string(entry)?,
                ])?;
                insert_fts.execute(params![
                    entry.id,
                    entry.name,
                    entry.summary,
                    entry.description,
                    entry.tags.join(" "),
                    entry.author,
                    entry.category,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn entry(&self, id: &str) -> Result<Option<Entry>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT json FROM entries WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(json.as_deref().and_then(decode))
    }

    pub fn search(&self, query: &Query) -> Result<Vec<Entry>> {
        let mut sql = String::from("SELECT e.json FROM entries e");
        let mut clauses: Vec<String> = Vec::new();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(fts) = &query.fts {
            sql.push_str(" JOIN entries_fts f ON f.id = e.id");
            clauses.push("entries_fts MATCH ?".into());
            args.push(fts.clone().into());
        }
        if let Some(kind) = query.kind {
            clauses.push("e.kind = ?".into());
            args.push(kind.id().to_string().into());
        }
        if let Some(category) = &query.category {
            clauses.push("e.category = ?".into());
            args.push(category.clone().into());
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(match (query.sort, query.fts.is_some()) {
            (Sort::Relevance, true) => {
                " ORDER BY bm25(entries_fts, 0, 10.0, 5.0, 1.0, 4.0, 2.0, 2.0), e.stars DESC"
            }
            (Sort::Relevance | Sort::Stars, _) => " ORDER BY e.stars DESC, e.name COLLATE NOCASE",
            (Sort::Newest, _) => " ORDER BY e.first_seen DESC, e.stars DESC",
            (Sort::Updated, _) => " ORDER BY e.updated_at DESC NULLS LAST, e.stars DESC",
            (Sort::Name, _) => " ORDER BY e.name COLLATE NOCASE",
        });
        sql.push_str(" LIMIT ? OFFSET ?");
        args.push((query.limit.max(1) as i64).into());
        args.push((query.offset as i64).into());
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(args), |row| {
            row.get::<_, String>(0)
        })?;
        let mut entries = Vec::new();
        for json in rows {
            if let Some(entry) = decode(&json?) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Everything else by the same developer, most starred first.
    pub fn by_author(&self, author_key: &str, except_id: &str) -> Result<Vec<Entry>> {
        if author_key.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self.conn.prepare(
            "SELECT json FROM entries WHERE author_key = ?1 AND id != ?2
             ORDER BY stars DESC, name COLLATE NOCASE",
        )?;
        let rows = statement.query_map(params![author_key, except_id], |row| {
            row.get::<_, String>(0)
        })?;
        Ok(rows
            .filter_map(|json| json.ok())
            .filter_map(|j| decode(&j))
            .collect())
    }

    /// Categories with their counts, biggest first; optionally one kind's.
    pub fn categories(&self, kind: Option<Kind>) -> Result<Vec<(String, usize)>> {
        let mut statement = self.conn.prepare(
            "SELECT category, COUNT(*) AS n FROM entries
             WHERE category != '' AND (?1 IS NULL OR kind = ?1)
             GROUP BY category ORDER BY n DESC, category",
        )?;
        let rows = statement.query_map(params![kind.map(Kind::id)], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The days something entered the catalog, newest first, with how many
    /// did: the Timeline's spine. `before` pages further back.
    pub fn timeline_days(
        &self,
        before: Option<NaiveDate>,
        limit: usize,
    ) -> Result<Vec<(NaiveDate, usize)>> {
        let mut statement = self.conn.prepare(
            "SELECT first_seen, COUNT(*) AS n FROM entries
             WHERE (?1 IS NULL OR first_seen < ?1)
             GROUP BY first_seen ORDER BY first_seen DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![before.map(|d| d.to_string()), limit as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize)),
        )?;
        let mut days = Vec::new();
        for row in rows {
            let (day, n) = row?;
            if let Ok(day) = day.parse::<NaiveDate>() {
                days.push((day, n));
            }
        }
        Ok(days)
    }

    pub fn entries_on(&self, day: NaiveDate) -> Result<Vec<Entry>> {
        let mut statement = self.conn.prepare(
            "SELECT json FROM entries WHERE first_seen = ?1 ORDER BY stars DESC, name COLLATE NOCASE",
        )?;
        let rows = statement.query_map(params![day.to_string()], |row| row.get::<_, String>(0))?;
        Ok(rows
            .filter_map(|json| json.ok())
            .filter_map(|j| decode(&j))
            .collect())
    }
}

fn decode(json: &str) -> Option<Entry> {
    match serde_json::from_str::<Entry>(json) {
        Ok(entry) => Some(entry),
        Err(error) => {
            // A row from an older model: skipped, replaced on the next index.
            tracing::warn!("a stored entry did not parse: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieve::{fts_any, fts_match};

    fn entry(id: &str, name: &str, summary: &str, author_key: &str, stars: i64) -> Entry {
        Entry {
            id: id.into(),
            kind: Kind::Plugin,
            subkind: "bar-widget".into(),
            name: name.into(),
            summary: summary.into(),
            description: String::new(),
            author: author_key.into(),
            author_key: author_key.into(),
            repo: String::new(),
            homepage: String::new(),
            listing_url: String::new(),
            license: String::new(),
            stars,
            category: "Hardware".into(),
            tags: vec!["battery".into()],
            version: String::new(),
            install: None,
            previews: Vec::new(),
            thumbnail: String::new(),
            first_seen: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            upstream_added: None,
            updated_at: None,
            wants_contributors: false,
            source: "test".into(),
            status: String::new(),
            verified: false,
        }
    }

    fn seeded() -> Database {
        let mut db = Database::open_in_memory().unwrap();
        let mut printer = entry("b", "Printy", "3D printer progress in the bar", "bob", 3);
        printer.first_seen = NaiveDate::from_ymd_opt(2026, 10, 2).unwrap();
        db.replace_entries(&[
            entry("a", "Batt", "Battery level and charging state", "alice", 10),
            printer,
            entry("c", "Charger", "Charging rate graph", "alice", 1),
        ])
        .unwrap();
        db
    }

    #[test]
    fn text_search_ranks_and_filters() {
        let db = seeded();
        let found = db
            .search(&Query {
                fts: fts_match("batter"),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        // "Batt" by name+summary, and both carry the `battery` tag.
        assert_eq!(found[0].id, "a");
        let sentence = db
            .search(&Query {
                fts: fts_any("something that shows my 3d printer progress"),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(sentence[0].id, "b");
        let none = db
            .search(&Query {
                fts: fts_match("zzz"),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn browsing_sorts_groups_and_pages_by_day() {
        let db = seeded();
        let all = db
            .search(&Query {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            all.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        let newest = db
            .search(&Query {
                sort: Sort::Newest,
                limit: 1,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(newest[0].id, "b");
        assert_eq!(db.by_author("alice", "a").unwrap()[0].id, "c");
        assert_eq!(
            db.categories(None).unwrap(),
            vec![("Hardware".to_string(), 3)]
        );
        let days = db.timeline_days(None, 10).unwrap();
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].1, 1);
        assert_eq!(db.entries_on(days[1].0).unwrap().len(), 2);
        let older = db.timeline_days(Some(days[0].0), 10).unwrap();
        assert_eq!(older.len(), 1);
        assert_eq!(db.count().unwrap(), 3);
        assert!(db.entry("nope").unwrap().is_none());
    }

    #[test]
    fn meta_round_trips() {
        let db = seeded();
        assert!(db.meta("etag").unwrap().is_none());
        db.set_meta("etag", "\"abc\"").unwrap();
        db.set_meta("etag", "\"def\"").unwrap();
        assert_eq!(db.meta("etag").unwrap().as_deref(), Some("\"def\""));
    }
}
