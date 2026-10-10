# Oscovery — task runner. `just` with no args lists recipes.

# Build the app (debug). Blueprints, resources and the schema compile via build.rs.
build:
    cargo build

# Build and run from the checkout. OSCOVERY_LOG=debug for verbose logging.
run *ARGS:
    cargo run -p oscovery -- {{ARGS}}

# clippy (warnings are errors) + rustfmt check + unit tests.
check:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo fmt --all -- --check
    cargo test --workspace

fmt:
    cargo fmt --all

# Build the catalog index from the marketplace into ./site (what the Actions job does).
index *ARGS:
    cargo run -p oscovery-index -- build --out site {{ARGS}}

# Install the release build for the current user (PREFIX=/usr for system-wide).
install:
    sh install.sh

# Remove what install put in place; `just uninstall --purge` removes your data too.
uninstall *args:
    sh uninstall.sh {{args}}

# Regenerate the translation template, po/oscovery.pot, from the sources.
pot:
    sh po/update-pot.sh

clean:
    cargo clean
