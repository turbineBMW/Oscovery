# Submitted projects

One TOML file per project that the topic crawl would not find on its own (no
`omarchy` topic, or below the quality floor for a reason the maintainer accepts).
The daily index build reads every `*.toml` here and classifies the repository
the same way as a crawled one.

```toml
repo = "https://github.com/owner/name"   # required; GitHub only for now
wants_contributors = true                # optional: the author is looking for help
category = "Apps"                        # optional override
```

To get a project in here, open an issue with the "Submit a project" form.
