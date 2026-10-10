# Submitted and removed projects

## Submitted

One TOML file per project that the daily build would not find on its own: no
`oscovery.toml` yet (code search can take a while to see a new one), no
`omarchy-plugin` / `omarchy-app` topic, or below the topic crawl's quality floor
for a reason the maintainer accepts. The build classifies each repository the same
way as a crawled one.

```toml
repo = "https://github.com/owner/name"   # required; GitHub only for now
submitted_by = "owner"                   # the GitHub login that opened the issue
wants_contributors = true                # optional: the author is looking for help
category = "Apps"                        # optional override
```

A submission is listed only when the person who submitted it owns the repository
(`submitted_by` matches the owner), or the repository opted in itself with an
`oscovery.toml` or an opt-in topic. Nobody can list someone else's project.

To get a project in here, open an issue with the "Submit a project" form.

## Removed

`removed.txt` keeps projects out whichever source found them (the marketplace, the
AUR, GitHub): one per line, as a repository URL, `owner/repo`, `owner/*` for
everything of one author, or a catalog id (`aur:name`, a plugin id). Authors ask
with the "Remove my project" form; the quicker way for a GitHub project is
`listed = false` in its `oscovery.toml`, which the next daily build honours.
