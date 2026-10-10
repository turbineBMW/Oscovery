//! Oscovery: find Omarchy apps and plugins by describing them.

mod accent;
mod application;
mod config;
mod dialogs;
mod i18n;
mod images;
mod omarchy;
mod rt;
mod settings;
mod widgets;
mod window;

use gtk::prelude::*;
use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    init_logging();
    i18n::init();

    // `oscovery import <index.json>`: load a local index build into the
    // catalog, for development and for an offline machine.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("import") {
        let Some(path) = args.get(2) else {
            eprintln!("usage: oscovery import <index.json>");
            return glib::ExitCode::FAILURE;
        };
        return import(std::path::Path::new(path));
    }

    gio::resources_register_include!("oscovery.gresource")
        .expect("the resource bundle is compiled in");
    glib::set_application_name("Oscovery");

    let app = application::OscoveryApplication::new();
    app.run()
}

/// Two logs: stderr at whatever `OSCOVERY_LOG` asks (warnings by default),
/// and `~/.cache/oscovery/oscovery.log` at debug, always, so a problem can
/// be looked into after the fact.
fn init_logging() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{EnvFilter, Layer};

    let stderr = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(
            EnvFilter::try_from_env("OSCOVERY_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        );
    let dir = glib::user_cache_dir().join(config::DIR_NAME);
    let _ = std::fs::create_dir_all(&dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("oscovery.log"))
        .ok();
    let file_layer = file.map(|file| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .with_filter(EnvFilter::new("oscovery=debug,oscovery_core=debug"))
    });
    tracing_subscriber::registry()
        .with(stderr)
        .with(file_layer)
        .init();
    tracing::info!("oscovery {} starting", config::VERSION);
}

fn import(path: &std::path::Path) -> glib::ExitCode {
    let index = match oscovery_core::index::read_file(path) {
        Ok(index) => index,
        Err(error) => {
            eprintln!("could not read {}: {error}", path.display());
            return glib::ExitCode::FAILURE;
        }
    };
    let mut db = match application::open_database() {
        Ok(db) => db,
        Err(error) => {
            eprintln!("{error}");
            return glib::ExitCode::FAILURE;
        }
    };
    if let Err(error) = db.replace_entries(&index.entries) {
        eprintln!("could not store the index: {error}");
        return glib::ExitCode::FAILURE;
    }
    let _ = db.set_meta("index-generated-at", &index.generated_at.to_rfc3339());
    let _ = db.set_meta("index-fetched-at", &chrono::Utc::now().to_rfc3339());
    let _ = db.set_meta("index-etag", "");
    println!("imported {} entries", index.entries.len());
    glib::ExitCode::SUCCESS
}
