# Oscovery

Find Omarchy apps and plugins by describing them.

Oscovery is a native GTK 4 / libadwaita app for [Omarchy](https://omarchy.org). It keeps a
local catalog of Omarchy plugins and apps whose authors put them out to be found
([plugins.omarchy.org](https://plugins.omarchy.org), GitHub projects that opted in, and the
AUR) and lets you find things three ways:

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

## For project authors

Oscovery only lists projects that were published to be found: plugins on
plugins.omarchy.org, AUR packages, and GitHub repositories that opted in. A repository that
merely mentions Omarchy is never listed without asking: plenty of them are someone's own setup.

**To be listed**, add an `oscovery.toml` at the root of your default branch. An empty file
is enough. The daily build looks for it in every repository that mentions Omarchy in its
name, description, README or topics and was pushed since the previous build, so it shows up
the day after you commit it. Tagging the repository `omarchy-plugin` or `omarchy-app` also
works, if it passes a basic quality floor (a description, a license, a push in the last 18
months). Every field is optional and corrects what Oscovery would otherwise guess:

```toml
summary = "One sentence on what it does"
category = "Media"
tags = ["audio", "waybar"]
wants_contributors = true                 # shows "Wants contributors" on its page
previews = ["https://.../screenshot.png"] # https only; the first is the card image
install = "install-script"                # or "github-release", "manual" (apps only)
install_note = "Needs Omarchy 3.2 or later."
```

Oscovery never takes a command from this file. The app builds the install command
from the method and shows it to the user before anything runs.

**To be left out**, whichever source found you (including the marketplace and the AUR):

```toml
listed = false
```

The next daily build drops it. Without a GitHub repository, or for everything you publish,
use the [Remove my project](https://github.com/turbineBMW/Oscovery/issues/new?template=remove-project.yml)
form. If your repository mentions Omarchy nowhere, add the file and use the
[Submit a project](https://github.com/turbineBMW/Oscovery/issues/new?template=submit-project.yml)
form once.

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
