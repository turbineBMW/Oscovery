//! GSettings, with the schema found either installed or in the build tree.

use crate::config::{APP_ID, BUILT_SCHEMA_DIR};
use gtk::gio;
use gtk::prelude::*;

pub const INDEX_URL: &str = "index-url";
pub const HARNESS: &str = "harness";
pub const MODEL: &str = "model";
#[allow(dead_code)]
pub const PROJECTS_DIR: &str = "projects-dir";
pub const FOLLOW_OMARCHY_THEME: &str = "follow-omarchy-theme";
pub const WINDOW_WIDTH: &str = "window-width";
pub const WINDOW_HEIGHT: &str = "window-height";
pub const WINDOW_MAXIMIZED: &str = "window-maximized";

/// The app's settings. Prefers the installed schema; falls back to the one
/// build.rs compiled so a checkout runs without `just install`.
pub fn load() -> gio::Settings {
    let installed =
        gio::SettingsSchemaSource::default().and_then(|source| source.lookup(APP_ID, true));
    match installed {
        Some(_) => gio::Settings::new(APP_ID),
        None => {
            let source = gio::SettingsSchemaSource::from_directory(BUILT_SCHEMA_DIR, None, false)
                .expect("build.rs compiled the schema");
            let schema = source
                .lookup(APP_ID, false)
                .expect("the schema names the app id");
            gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None)
        }
    }
}

/// The index URL in force: `OSCOVERY_INDEX` (a path or URL, for development)
/// outranks the setting.
pub fn index_url(settings: &gio::Settings) -> String {
    std::env::var("OSCOVERY_INDEX")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| settings.string(INDEX_URL).to_string())
}
