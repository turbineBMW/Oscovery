# CLAUDE.md

Oscovery is a native GTK 4 / libadwaita app for Omarchy, in Rust: find Omarchy apps and
plugins by describing them. The user's own coding agent (Claude Code, Codex, opencode, pi)
searches a local catalog and answers; Browse and a day-by-day Timeline work without one.
App id `io.github.turbinebmw.Oscovery`, data dir `$XDG_DATA_HOME/oscovery`, cache
`$XDG_CACHE_HOME/oscovery`. The plan and the research behind it are in PLAN.md.

## Build & run

No Flatpak, no meson. `cargo run -p oscovery` works from a checkout: `build.rs` compiles the
Blueprints into a GResource and the GSettings schema into `OUT_DIR`, and `settings::load`
falls back to that compiled schema when the app isn't installed.

```
just run           # cargo run -p oscovery
just check         # clippy -D warnings + rustfmt --check + cargo test  (keep it clean)
just index         # build the catalog index into ./site from the marketplace (network)
just install       # sh install.sh -> ~/.local (PREFIX=/usr for system-wide)
```

Host deps: gtk4 ≥ 4.18, libadwaita ≥ 1.8, blueprint-compiler, glib2 (glib-compile-schemas).
sqlite is bundled by rusqlite; TLS is rustls; images decode in-process (`image` crate, so
WebP needs no pixbuf loader). Edition 2024, MSRV 1.88.

`OSCOVERY_LOG=debug` (any `tracing` EnvFilter) turns stderr logging up;
`~/.cache/oscovery/oscovery.log` always has debug.

Dev hooks: `OSCOVERY_INDEX=<path or URL>` overrides the index source (a checkout's
`site/index.json` after `just index`); `oscovery import <index.json>` loads one into the
catalog without the GUI; `OSCOVERY_DEBUG_SECTION=discover|browse|timeline` and
`OSCOVERY_DEBUG_OPEN=<entry id>` open there at launch; `oscovery ask "<question>"` runs one
Discover question headlessly and prints the parsed answer (the best way to iterate on
`harness::INSTRUCTIONS`). Note `env -u CLAUDECODE` when testing from inside Claude Code. For a screenshot on Hyprland:
`hyprctl clients -j` for the window geometry (class `io.github.turbinebmw.Oscovery`), `grim -g`.

## Layout

```
Cargo.toml                 workspace (edition 2024, thin LTO in release)
crates/oscovery-core/src/  no widgets; everything here is unit-tested (`cargo test`)
  models.rs    Entry (one plugin or app), Kind, Install/InstallMethod, Index; github_slug
  db.rs        rusqlite + FTS5: entries + entries_fts, meta; search(Query), by_author,
               categories, timeline_days/entries_on. Append-only MIGRATIONS (user_version)
  index.rs     fetch index.json with an ETag (304 -> NotModified); file:// and paths too
  retrieve.rs  fts_match (every word required) / fts_any (a sentence: any word, no stop words)
  harness.rs   Harness (claude|codex|opencode|pi): headless read-only invocation, blocking
               `ask`, the INSTRUCTIONS, search_prompt (transcript re-sent whole), parse_answer
               (prose + the ```oscovery JSON block), interactive_args + extend_prompt (hand-off)
  workdir.rs   the catalog directory the agent greps: INDEX.md + catalog/<kind>/<id>.md,
               rebuilt when the index stamp changes
  install.rs   install_action (omarchy-plugin-add --yes | omarchy-plugin-enable | yay in a
               terminal | manual), enable/disable/remove, run, installed_plugins (--json)
  review.rs    security review: source_url, shallow clone, prompt, parse (verdict + findings)
  omarchy.rs   Omarchy colors.toml -> libadwaita CSS variables (ported from Skali/Rustle)
crates/oscovery-index/     the crawler: `oscovery-index build --out site` reads the
                           marketplace catalog.json, keeps state/first_seen.json (the
                           Timeline's axis; seeded from addedAt on a fresh state), writes
                           index.json, timeline.json, manifest.json. Runs in GitHub Actions.
crates/oscovery/src/       the GTK layer
  main.rs      logging, gettext, `import`, run;  application.rs  db + settings, actions,
               refresh_index (fetch -> replace_entries -> window.catalog_changed)
  rt.rs        the tokio runtime and `rt::run(job, on_done)` -- the threading model (below)
  agent.rs     asking from GTK: configured harness, workdir stamp, shortlist, ask (job on
               tokio, parsed answer + resolved entries back), clone_into + hand_off (terminal)
  window/      mod.rs (sidebar, sections, open_entry/open_author, debug hooks),
               browse.rs (search + filters, paged FlowBox), timeline.rs (days on scroll),
               project.rs (ProjectPage: install/enable/disable/remove with a confirm
               showing the command, more by author, similar), author.rs, review.rs (the
               review page), discover.rs (the conversation: bubbles, match cards, extend)
  widgets/     entry_card.rs (EntryCard template), mod.rs (chips, fill_cards, initials)
  images.rs    preview fetch -> ~/.cache/oscovery/images, decode off-thread -> MemoryTexture
  dialogs/     preferences.rs (agent, model, index URL, theme)
  ui/*.blp     Blueprint templates; the Rust attribute names must match the ids
  settings.rs, config.rs, i18n.rs, accent.rs, omarchy.rs (theme following, from Skali)
data/                      gschema, desktop file, metainfo, D-Bus service, icons
```

## Rules

- **Threading:** all network and subprocess I/O goes through `rt::run(job, on_done)`: `job`
  is a `Send` future on the tokio runtime with owned data; `on_done` runs on the main loop
  and is the only place that touches the database or widgets. Harness output streams back
  over an `async_channel`.
- **The catalog is replaced whole** (`replace_entries` in one transaction); nothing edits
  rows. Local state that must survive an index refresh (conversations, reviews, installed
  state) goes in its own tables.
- **Install never runs silently:** the exact command is shown on the project page before
  anything runs; plugins go through `omarchy plugin add` (a clone, no code executed, no
  sudo); anything needing root runs in a floating terminal so the user sees the prompt.
- **The agent runs on the user's account, read-only, in a scratch directory** with only
  the catalog files in it. Nothing from the user's home is passed in; nothing it writes is
  kept. Clone-and-extend hands off to the agent *interactively* in a terminal.
- **CSS colours** come from libadwaita variables (`var(--accent-bg-color)`) so Omarchy
  themes reach them. A wrapping label inside a card needs `max-width-chars`, or its
  natural width is the whole text and the grid collapses to one card per row.
- **DB migrations** are append-only.
- Commits: Conventional Commits, terse.
