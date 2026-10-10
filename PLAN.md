# Oscovery — find Omarchy apps and plugins by describing them

Plan written 2026-10-10 from a survey of Rustle, Skald, Skali and Bubo (house
style), the Omarchy 4.0 plugin tooling on this machine, OmaStore
(KitsuneForgering), the plugins.omarchy.org marketplace and its data, and the
GitHub topic landscape.

## 1. What we are building

A native GTK4/libadwaita app (Rust, Omarchy-themed, house style) that answers
"is there an Omarchy app or plugin that does X?" without the user knowing its
name. The user plugs in the coding agent they already have (Claude Code, Codex,
opencode, pi; more later) and describes what they want. The agent searches a
local catalog, answers with matches, and the conversation continues ("I need it
to do Y" → "these two do"). When nothing matches, the agent offers to clone the
closest project, add the feature and open a PR, so effort flows into existing
projects instead of a ninth email client.

Not a store, not a replacement for plugins.omarchy.org: Oscovery links there,
reuses its data, and sends plugin submissions through its issue forms.

Pages:
- **Discover** — the prompt box ("I want a bar widget that shows my 3D printer
  progress"), results as cards, conversation continues below.
- **Browse** — classic search/filter by kind, category, tags, author; same cards.
- **Timeline** — what entered the catalog each day, scroll day by day.
- **Project** — README, screenshots, stars, license, kind, install state,
  **Install** / **Review for security, then install**, "More by this developer",
  "Similar projects", "Wants contributors" flag, link to upstream and to
  plugins.omarchy.org.
- **Preferences** — harness and model, theme following, index source.

## 2. Key facts (decisions hang on these)

- **Omarchy plugins** (4.0 "shell plugins"): `manifest.json` at repo root
  (`schemaVersion: 1`, `id`, `name`, `version`, `kinds[]`, `entryPoints{}`,
  optional `author`, `license`, `description`, `barWidget{category, tags…}`).
  Install is `omarchy plugin add <git-url> [--enable] [--yes]`: a git clone into
  `~/.config/omarchy/plugins/<id>/` after `omarchy-plugin-validate`; **no sudo,
  no code executed**. `omarchy-plugin-list --json` gives installed ids +
  `enabled`; `omarchy-plugin-catalog` gives full manifests. Update/remove/
  enable/disable are all `omarchy-plugin-*` commands.
- **Apps**: no Omarchy-native format. Sources are AUR packages (yay is present;
  `omarchy-pkg-aur-install`, `omarchy pkg add` for repo packages), GitHub
  releases (OmaStore's model: `omastore.toml`, downloads into `$HOME`), and
  source checkouts with an `install.sh` (this author's own apps). Anything
  needing root goes through a floating terminal
  (`omarchy-launch-floating-terminal-with-presentation`), the Omarchy way.
- **plugins.omarchy.org** = `omacom/omarchy-plugin-marketplace` (MIT). Static
  site + one Cloudflare Worker (D1) for view/copy/heart counts. **Public
  `https://plugins.omarchy.org/catalog.json`** (13.5 MB): 5,343 plugins with
  `id, name, description, author, version, category, tags[], kind, repo,
  installCommand, license, stars, addedAt, listedAt, repositoryUpdatedAt,
  previewImage, verificationStatus…`. Submissions are GitHub issue forms
  (`submit-plugin.yml`), not PRs. Categories: Widgets 1459, Productivity 1062,
  System 858, Hardware 506, Desktop 481, Dev Tools 407, Appearance 402…
- **OmaStore**: Go daemon + Qt GUI, apps only, 4 apps in its catalog today.
  Good ideas to borrow: `omastore.toml` manifest, sha256 + Sigstore checks on
  release assets, category from GitHub topics. Not a data source.
- **GitHub**: topic `omarchy-plugin` 1,318 repos, topic `omarchy` 3,854 repos.
  Search API is 10 req/min anonymous, 30/min with a token; core REST 5,000/h
  with a token. Fine for a daily crawl in Actions, not for clients.
- **Harness CLIs** are all installed here (claude, codex, pi, opencode, gemini,
  cursor-agent). Rustle's `assistant.rs` already knows the headless one-shot
  invocation for four of them (`claude -p`, `codex exec`, `opencode run`,
  `pi -p`), finds them on PATH or `~/.local/bin`, and kills after a timeout.
  What Oscovery adds: tools *on* but confined to a scratch directory, streaming
  output, and multi-turn.

## 3. The cheapest scalable backend: no backend

| Layer | Choice | Cost | Why |
|---|---|---|---|
| Catalog build | GitHub Actions cron (daily) in a public repo `oscovery-index` | $0 (public repos: unlimited minutes) | Crawls catalog.json, GitHub topics, AUR RPC, OmaStore; merges; writes `index.json` |
| Catalog hosting | GitHub Pages (or release asset) + ETag | $0 | CDN-fronted, `If-None-Match` → 304. ~2 MB gz ~400 KB; 10k users/day ≈ 4 GB/mo, far under the 100 GB soft cap. Cloudflare in front later if ever needed, still $0 |
| Search | Local: rusqlite FTS5 (BM25) on the client | $0 | Instant, offline, no server. Semantic understanding comes from the user's own model |
| "Semantic" search | The user's harness, running on their account | $0 to us | Two-tier: FTS shortlist + the agent greps the full catalog dir itself |
| Submissions | GitHub issue forms on `oscovery-index`; plugins forwarded to the marketplace's `submit-plugin.yml` | $0 | No auth to build; `gh` or a prefilled URL from the app |
| Engagement (optional, later) | Cloudflare Worker + D1 free tier, same as the marketplace | $0 to 100k req/day | Only if we want hearts/install counts; not in v1 |

Rejected: a hosted database/API (Supabase, Fly, VPS) buys nothing v1 needs and
is the one thing that would cost money or go down; hosted embeddings/vector
search ditto, since the agent does the reasoning.

Scaling path if the index outgrows one file: shard `index.json` per kind, keep a
small `manifest.json` with shard ETags. Not needed below ~20k entries.

## 4. Data model

One **entry** per project (plugin or app):

```
id            "io.github.user.name" (plugin id) | "aur:name" | "gh:owner/repo"
kind          plugin | app
subkind       bar-widget|panel|overlay|menu|service|bar | aur|github-release|source
name, summary, description (README excerpt, ≤2 KB), readme_url
author        display name;  author_key  GitHub owner or AUR maintainer (groups "more by")
repo, homepage, license, stars, category, tags[]
install       { method: omarchy-plugin|aur|pacman|github-release|install-sh, command, note }
previews[]    image URLs
first_seen    date we first indexed it   (drives the Timeline)
upstream_added  marketplace addedAt / repo created_at (shown too)
updated_at    last upstream push/release
wants_contributors  bool (from `.oscovery.toml`, a GitHub topic `omarchy-wants-contributors`, or the submission form)
source        marketplace | github-topic | aur | omastore | submitted
```

Client DB (`$XDG_DATA_HOME/oscovery/oscovery.sqlite`): `entries` + FTS5 over
name/summary/description/tags/author, `installed` (reconciled from
`omarchy-plugin-list --json` and `pacman -Q`), `conversations`, `reviews`.
Append-only migrations via `PRAGMA user_version` (Rustle `db.rs`).

## 5. The agent loop

1. User types a request on Discover.
2. `oscovery-core::retrieve` runs FTS over the prompt (plus synonyms expansion
   of the obvious: "bar" → bar-widget etc.) → top ~60 candidates.
3. A scratch workdir is built: `catalog/<kind>/<id>.md` for **every** entry
   (frontmatter + summary + README excerpt; regenerated from sqlite when the
   index changes, ~5 k small files), `SHORTLIST.md` (the 60 candidates in
   full), `INSTRUCTIONS.md` (the system prompt: you are searching a catalog,
   grep `catalog/` freely, answer in prose then a fenced `oscovery` JSON block
   `{"matches":[{"id","why"}], "offer": "none"|"extend"|"build"}`).
4. The harness runs headless with **read-only tools confined to the workdir**:
   - claude: `-p --allowedTools Read,Grep,Glob --permission-mode default --add-dir <workdir> --no-session-persistence --setting-sources "" --strict-mcp-config --output-format stream-json`
   - codex: `exec --sandbox read-only --skip-git-repo-check --ephemeral --json`
   - opencode: `run` in the workdir (its tools read the dir)
   - pi: `-p` with tools in the workdir
   Output streams line by line into the conversation view.
5. The JSON block is parsed; matched ids render as cards under the answer.
6. Follow-ups re-send the transcript (harness-agnostic); `--resume` /
   `codex exec resume` as an optimisation later.
7. `"offer": "extend"` renders a **Clone and add this feature** button: clones
   the project into `~/Projects/<name>` (configurable) and opens a terminal
   with the harness *interactively*, seeded with the user's feature request and
   a "open a PR when done" instruction. The agent's full tools belong in the
   user's terminal, not in Oscovery.

Security review: clone into `~/.cache/oscovery/review/<id>`, run the harness
headless with read-only tools over the clone, prompt from `REVIEW.md` (what to
look for: network calls, `curl | sh`, credential access, obfuscation, sudo,
post-install hooks, PKGBUILD `source=` and checksums for AUR), stream to a
review page, parse a final verdict block (`safe | caution | avoid`, findings).
**Install** stays one click away with the verdict shown next to it; the review
never installs anything itself.

## 6. Install

| Method | Action | Elevation |
|---|---|---|
| omarchy-plugin | `omarchy plugin add <repo> --enable --yes` as a child process, then `rescanPlugins` happens inside | none |
| aur | floating terminal: `yay -S --needed <pkg>` (via `omarchy-pkg-aur-install` where it fits) | the terminal prompts |
| pacman | floating terminal: `omarchy pkg add <pkg>` | the terminal prompts |
| github-release | v2: OmaStore-style download into `$HOME` with sha256 check; v1: open the release page + show the asset | none |
| install-sh | floating terminal: `git clone` then `sh install.sh` (shown before running) | the script prompts |

Installed state: plugins from `omarchy-plugin-list --json` (enable/disable/
remove/update buttons map to `omarchy-plugin-*`), packages from `pacman -Q`.

## 7. Architecture (house style)

```
Cargo.toml                     workspace, edition 2024, thin LTO
crates/oscovery-core/src/      no widgets; unit-tested
  models.rs     Entry, Install, Kind, Harness verdicts
  db.rs         rusqlite + FTS5, migrations, installed reconciliation
  index.rs      fetch index.json (reqwest, ETag), apply diff, timeline days
  retrieve.rs   FTS query building, shortlist, synonym map
  workdir.rs    scratch catalog/ + SHORTLIST.md + INSTRUCTIONS.md generation
  harness.rs    Harness enum (from Rustle's assistant.rs) + streaming runner
                with read-only tool confinement + transcript re-send
  review.rs     clone, REVIEW.md prompt, verdict parsing
  install.rs    install plans → commands; omarchy-plugin-* wrappers
  omarchy.rs    Omarchy palette → libadwaita CSS (Skald's, verbatim)
crates/oscovery/src/           GTK layer
  main.rs, application.rs, settings.rs (GSettings compiled into OUT_DIR),
  rt.rs (tokio + run(job, on_done)), accent.rs, omarchy.rs
  window/       mod.rs (State), discover.rs, browse.rs, timeline.rs,
                project.rs, preferences.rs
  widgets/      entry_card.rs, chat_view.rs (streamed markdown), verdict.rs
  objects.rs    GObject wrappers over Entry for ListStore/GridView
  ui/*.blp      Blueprint templates
crates/oscovery-index/         the crawler/merger, run by GitHub Actions in
                               the `oscovery-index` repo (same binary, `--out`)
data/                          gschema, desktop, metainfo, icons
install.sh / uninstall.sh / justfile   from Rustle
```

Threading: all network, subprocess and keyring I/O through `rt::run(job,
on_done)`; `on_done` on the main loop is the only thing touching DB or widgets.
Harness output streams through an `async_channel` into the chat view.

## 8. The index repo (`oscovery-index`)

- `sources/` — hand-curated additions and the submission queue (`*.toml`,
  one per project; issue form → Action → PR → merged).
- `.github/workflows/build.yml` — daily cron + on push: runs
  `oscovery-index build --out site/`, commits `first_seen` memory
  (`state/first_seen.json`), publishes `site/` to Pages.
- Crawl: marketplace `catalog.json` (all plugins), GitHub topics `omarchy`,
  `omarchy-plugin`, `omarchy-app` (apps: repos with a desktop file,
  `omastore.toml`, `install.sh`, or a PKGBUILD, minus plugins already seen),
  AUR RPC `search?arg=omarchy` + maintainers of known packages, OmaStore
  `catalog.json`. Blocklist file. Stars/updated_at refreshed daily with a token.
- Output: `index.json` (+ `.gz`), `timeline.json` (day → ids), `manifest.json`
  (version, counts, sha256).

## 9. Phases

Status 2026-10-10 (evening): 1, 2, 3, 4 and 5 are implemented and committed; 6 is
done except submissions: the workflow in `.github/workflows/index.yml` crawls
the marketplace, GitHub topics and the AUR (needs GitHub Pages switched to
"GitHub Actions" once, in the repository settings). Submission issue forms,
and 7, are open.

1. **Foundation** (this session): workspace, theme following, settings, DB +
   FTS, index fetch from a local file, Browse with search and cards, Project
   page with upstream links. Index builder that converts catalog.json → our
   schema (so there is real data on day one).
2. **Install**: plugin install/enable/disable/remove via `omarchy-plugin-*`,
   AUR/pacman via floating terminal, installed-state reconciliation.
3. **Discover**: workdir generation, harness runner with streaming, chat view,
   result cards, follow-ups, "clone and add" hand-off.
4. **Timeline** and **More by this developer**, similar projects.
5. **Security review** page.
6. **Index repo**: Actions workflow, GitHub topic + AUR crawl, submission
   issue forms, Pages publish; app switches from local file to the URL.
7. Polish: metainfo, screenshots, README, contributors flag, `omastore.toml`
   support for GitHub-release installs.

## 10. Open questions (defaults in bold)

- Index repo name/location: **`turbinebmw/oscovery-index`, GitHub Pages**.
- Apps from the `omarchy` topic: **include automatically** with a quality floor
  (has README + license, pushed within 18 months) and a blocklist, since
  discovery is the whole point; curated-only would miss exactly the apps nobody
  markets.
- Default harness: **whichever of claude/codex/opencode/pi is installed, in that
  order**; ask in Preferences otherwise.
