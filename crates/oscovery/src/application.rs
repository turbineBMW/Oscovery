//! The application: one database and settings object shared by every
//! window, the app-level actions, and the catalog refresh.

use crate::config::{APP_ID, DIR_NAME, RESOURCE_PATH, VERSION};
use crate::dialogs::preferences::PreferencesDialog;
use crate::i18n::{failure_message, gettext};
use crate::omarchy::OmarchyTheme;
use crate::window::OscoveryWindow;
use crate::{rt, settings};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::gio;
use gtk::glib;
use oscovery_core::db::Database;
use oscovery_core::error::Result;
use oscovery_core::index::{self, Fetched};
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

/// How old a catalog may be before launch refreshes it.
const REFRESH_AFTER: chrono::Duration = chrono::Duration::hours(12);

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct OscoveryApplication {
        pub db: OnceCell<Rc<RefCell<Database>>>,
        pub settings: OnceCell<gio::Settings>,
        pub omarchy_theme: OnceCell<OmarchyTheme>,
        pub refreshing: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OscoveryApplication {
        const NAME: &'static str = "OscoveryApplication";
        type Type = super::OscoveryApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for OscoveryApplication {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_actions();
        }
    }

    impl ApplicationImpl for OscoveryApplication {
        fn startup(&self) {
            self.parent_startup();
            let app = self.obj();
            app.open();
            app.load_css();
        }

        fn activate(&self) {
            let app = self.obj();
            let window = app.main_window().unwrap_or_else(|| app.new_window());
            window.present();
            app.refresh_if_stale();
        }
    }
    impl GtkApplicationImpl for OscoveryApplication {}
    impl AdwApplicationImpl for OscoveryApplication {}
}

glib::wrapper! {
    pub struct OscoveryApplication(ObjectSubclass<imp::OscoveryApplication>)
        @extends adw::Application, gtk::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Default for OscoveryApplication {
    fn default() -> Self {
        Self::new()
    }
}

/// The catalog database under the user's data directory.
pub fn open_database() -> Result<Database> {
    let data_dir = glib::user_data_dir().join(DIR_NAME);
    std::fs::create_dir_all(&data_dir)?;
    Database::open(&data_dir.join("oscovery.db"))
}

impl OscoveryApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", APP_ID)
            .property("resource-base-path", RESOURCE_PATH)
            .build()
    }

    pub fn db(&self) -> Rc<RefCell<Database>> {
        self.imp().db.get().expect("opened at startup").clone()
    }

    pub fn settings(&self) -> gio::Settings {
        self.imp()
            .settings
            .get()
            .expect("loaded at startup")
            .clone()
    }

    fn open(&self) {
        let db =
            open_database().unwrap_or_else(|error| panic!("could not open the catalog: {error}"));
        let _ = self.imp().db.set(Rc::new(RefCell::new(db)));
        let _ = self.imp().settings.set(settings::load());
    }

    pub(crate) fn new_window(&self) -> OscoveryWindow {
        OscoveryWindow::new(self, self.db(), &self.settings())
    }

    pub(crate) fn main_window(&self) -> Option<OscoveryWindow> {
        self.windows()
            .into_iter()
            .find_map(|window| window.downcast::<OscoveryWindow>().ok())
    }

    fn load_css(&self) {
        let Some(display) = gtk::gdk::Display::default() else {
            return;
        };
        let provider = gtk::CssProvider::new();
        provider.load_from_resource(&format!("{RESOURCE_PATH}/style.css"));
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        crate::accent::install_fallback(&display);
        let _ = self
            .imp()
            .omarchy_theme
            .set(OmarchyTheme::install(&display, &self.settings()));
    }

    fn setup_actions(&self) {
        let about = gio::ActionEntry::builder("about")
            .activate(|app: &Self, _, _| app.show_about())
            .build();
        let quit = gio::ActionEntry::builder("quit")
            .activate(|app: &Self, _, _| app.quit())
            .build();
        let preferences = gio::ActionEntry::builder("preferences")
            .activate(|app: &Self, _, _| {
                app.activate();
                if let Some(window) = app.main_window() {
                    PreferencesDialog::new(app, &window).present(Some(&window));
                }
            })
            .build();
        let refresh = gio::ActionEntry::builder("refresh")
            .activate(|app: &Self, _, _| app.refresh_index(true))
            .build();
        self.add_action_entries([about, quit, preferences, refresh]);

        for (name, accels) in [
            ("app.quit", &["<control>q"][..]),
            ("app.preferences", &["<control>comma"]),
            ("app.refresh", &["F5"]),
            ("win.search", &["<control>f"]),
            ("win.go::discover", &["<alt>1"]),
            ("win.go::browse", &["<alt>2"]),
            ("win.go::timeline", &["<alt>3"]),
        ] {
            self.set_accels_for_action(name, accels);
        }
    }

    /// Fetch the catalog at launch when it is empty or older than
    /// `REFRESH_AFTER`.
    fn refresh_if_stale(&self) {
        let db = self.db();
        let (count, fetched_at) = {
            let db = db.borrow();
            (
                db.count().unwrap_or(0),
                db.meta("index-fetched-at").ok().flatten(),
            )
        };
        let fresh = fetched_at
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok())
            .is_some_and(|at| chrono::Utc::now() - at.with_timezone(&chrono::Utc) < REFRESH_AFTER);
        if count == 0 || !fresh {
            self.refresh_index(false);
        }
    }

    /// Fetch the index unless the server says ours is current, store it,
    /// and tell the window. `announce` toasts the outcome either way; a
    /// background refresh only toasts a failure when the catalog is empty.
    pub fn refresh_index(&self, announce: bool) {
        if self.imp().refreshing.replace(true) {
            return;
        }
        let db = self.db();
        let url = settings::index_url(&self.settings());
        let etag = db
            .borrow()
            .meta("index-etag")
            .ok()
            .flatten()
            .filter(|e| !e.is_empty());
        let empty = db.borrow().count().unwrap_or(0) == 0;
        if let Some(window) = self.main_window() {
            window.set_refreshing(true);
        }
        tracing::info!("fetching the catalog from {url}");
        let fetch_url = url.clone();
        rt::run(
            async move { index::fetch(&fetch_url, etag.as_deref()).await },
            glib::clone!(
                #[weak(rename_to = app)]
                self,
                move |result| {
                    app.imp().refreshing.set(false);
                    let window = app.main_window();
                    if let Some(window) = &window {
                        window.set_refreshing(false);
                    }
                    match result {
                        Ok(Fetched::NotModified) => {
                            let _ = db
                                .borrow()
                                .set_meta("index-fetched-at", &chrono::Utc::now().to_rfc3339());
                            if announce && let Some(window) = &window {
                                window.toast(&gettext("The catalog is up to date."));
                            }
                        }
                        Ok(Fetched::Index { index, etag }) => {
                            let stored = {
                                let mut db = db.borrow_mut();
                                db.replace_entries(&index.entries).and_then(|()| {
                                    db.set_meta("index-etag", etag.as_deref().unwrap_or(""))?;
                                    db.set_meta("index-url", &url)?;
                                    db.set_meta(
                                        "index-generated-at",
                                        &index.generated_at.to_rfc3339(),
                                    )?;
                                    db.set_meta(
                                        "index-fetched-at",
                                        &chrono::Utc::now().to_rfc3339(),
                                    )
                                })
                            };
                            match stored {
                                Ok(()) => {
                                    tracing::info!("stored {} entries", index.entries.len());
                                    if let Some(window) = &window {
                                        window.catalog_changed();
                                        if announce {
                                            window.toast(&crate::i18n::format(
                                                &gettext("Catalog updated: {n} projects."),
                                                &[("n", &index.entries.len().to_string())],
                                            ));
                                        }
                                    }
                                }
                                Err(error) => {
                                    tracing::error!("could not store the catalog: {error}");
                                    if let Some(window) = &window {
                                        window.toast(&failure_message(&error));
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            tracing::warn!("could not fetch the catalog from {url}: {error}");
                            if (announce || empty)
                                && let Some(window) = &window
                            {
                                window.toast(&failure_message(&error));
                            }
                        }
                    }
                }
            ),
        );
    }

    fn show_about(&self) {
        let about =
            adw::AboutDialog::from_appdata(&format!("{RESOURCE_PATH}/metainfo.xml"), Some(VERSION));
        about.set_translator_credits(&gettext("translator-credits"));
        about.set_developers(&["Brandon Williams"]);
        about.set_copyright("© 2026 Brandon Williams");
        about.present(self.active_window().as_ref());
    }
}
