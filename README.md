# Oscovery

Find Omarchy apps and plugins by describing them.

Oscovery is a native GTK 4 / libadwaita app for [Omarchy](https://omarchy.org). It keeps a
local catalog of every known Omarchy plugin and app (seeded from
[plugins.omarchy.org](https://plugins.omarchy.org), with GitHub and the AUR to follow) and
lets you find things three ways:

- **Discover** — type what you want ("a bar widget that shows my 3D printer's progress")
  and the coding agent you already have (Claude Code, Codex, opencode, pi) searches the
  catalog and answers, then keeps the conversation going. When nothing fits, it can offer
  to clone the closest project, add the feature, and open a pull request, so effort goes
  into existing projects instead of a ninth email client.
- **Browse** — search and filter by kind, category, tag and author.
- **Timeline** — what entered the catalog, day by day.

Every project page shows how it installs (the exact command, before anything runs), links
upstream, lists **more by the same developer**, and similar projects. Plugins install with
`omarchy plugin add`; packages open a terminal so you see the prompt. A **Review, then
install** button has your agent read the source first.

Oscovery is not a store and does not replace plugins.omarchy.org: it reuses that catalog
and links back to it.

## The catalog

There is no server. A public repository builds one `index.json` daily in GitHub Actions
(`crates/oscovery-index`) and publishes it on GitHub Pages; the app fetches it with an ETag
and searches it locally with SQLite FTS5. The "semantic" part is your own agent, on your own
account.

## Running

```sh
cargo run -p oscovery                           # from a checkout
cargo run -p oscovery-index -- build --out site # build the index locally (network)
OSCOVERY_INDEX=$PWD/site/index.json cargo run -p oscovery
sh install.sh                                   # ~/.local; PREFIX=/usr for system-wide
```

Host dependencies: gtk4 ≥ 4.18, libadwaita ≥ 1.8, blueprint-compiler, glib2.

## Omarchy themes

Oscovery takes its colours, accent and light/dark mode from the active Omarchy theme and
follows `omarchy theme set` live. A theme can take full control with an `oscovery.css`.

## License

GPL-3.0-or-later.
