//! The main window: Discover, Browse and Timeline in the sidebar, the
//! section on the right, project pages pushed over it.

mod author;
mod browse;
mod discover;
mod project;
mod timeline;

use crate::application::OscoveryApplication;
use crate::i18n::{format, gettext, plural};
use crate::settings as keys;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::gio;
use gtk::glib;
use oscovery_core::db::Database;
use oscovery_core::models::Entry;
use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

pub use browse::BrowsePage;
pub use discover::DiscoverPage;
pub use project::ProjectPage;
pub use timeline::TimelinePage;

const SECTIONS: [&str; 3] = ["discover", "browse", "timeline"];

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/turbinebmw/Oscovery/ui/main-window.ui")]
    pub struct OscoveryWindow {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub split: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub sidebar_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub stats_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub content_nav: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub section_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub refresh_spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        pub section_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub discover_page: TemplateChild<DiscoverPage>,
        #[template_child]
        pub browse_page: TemplateChild<BrowsePage>,
        #[template_child]
        pub timeline_page: TemplateChild<TimelinePage>,

        pub db: OnceCell<Rc<RefCell<Database>>>,
        pub settings: OnceCell<gio::Settings>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OscoveryWindow {
        const NAME: &'static str = "OscoveryWindow";
        type Type = super::OscoveryWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            DiscoverPage::ensure_type();
            BrowsePage::ensure_type();
            TimelinePage::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for OscoveryWindow {}
    impl WidgetImpl for OscoveryWindow {}
    impl WindowImpl for OscoveryWindow {
        fn close_request(&self) -> glib::Propagation {
            self.obj().save_geometry();
            self.parent_close_request()
        }
    }
    impl ApplicationWindowImpl for OscoveryWindow {}
    impl AdwApplicationWindowImpl for OscoveryWindow {}
}

glib::wrapper! {
    pub struct OscoveryWindow(ObjectSubclass<imp::OscoveryWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl OscoveryWindow {
    pub fn new(
        app: &OscoveryApplication,
        db: Rc<RefCell<Database>>,
        settings: &gio::Settings,
    ) -> Self {
        let window: Self = glib::Object::builder().property("application", app).build();
        let imp = window.imp();
        let _ = imp.db.set(db);
        let _ = imp.settings.set(settings.clone());

        window.set_default_size(
            settings.int(keys::WINDOW_WIDTH),
            settings.int(keys::WINDOW_HEIGHT),
        );
        if settings.boolean(keys::WINDOW_MAXIMIZED) {
            window.maximize();
        }

        window.setup_actions();
        imp.discover_page.setup(&window);
        imp.browse_page.setup(&window);
        imp.timeline_page.setup(&window);

        imp.sidebar_list.connect_row_selected(glib::clone!(
            #[weak]
            window,
            move |_, row| {
                if let Some(row) = row {
                    window.show_section(&row.widget_name());
                }
            }
        ));
        // Browse is where the catalog is; Discover becomes the first page
        // once the agent is wired up.
        if let Some(row) = imp.sidebar_list.row_at_index(1) {
            imp.sidebar_list.select_row(Some(&row));
        }
        window.catalog_changed();
        // Once the window is up: a page pushed before the first frame is
        // not shown.
        glib::idle_add_local_once(glib::clone!(
            #[weak]
            window,
            move || window.apply_debug_hooks()
        ));
        window
    }

    /// `OSCOVERY_DEBUG_SECTION=timeline` shows that section at launch;
    /// `OSCOVERY_DEBUG_OPEN=<entry id>` opens that project's page. For
    /// screenshots and for looking at one page without clicking there.
    fn apply_debug_hooks(&self) {
        if let Ok(section) = std::env::var("OSCOVERY_DEBUG_SECTION") {
            self.select_section(&section);
        }
        if let Ok(id) = std::env::var("OSCOVERY_DEBUG_OPEN") {
            let entry = self.db().borrow().entry(&id).ok().flatten();
            match entry {
                Some(entry) => self.open_entry(&entry),
                None => tracing::warn!("OSCOVERY_DEBUG_OPEN: no entry {id}"),
            }
        }
    }

    pub(crate) fn db(&self) -> Rc<RefCell<Database>> {
        self.imp().db.get().expect("set in new").clone()
    }

    pub(crate) fn settings(&self) -> &gio::Settings {
        self.imp().settings.get().expect("set in new")
    }

    fn setup_actions(&self) {
        let go = gio::ActionEntry::builder("go")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|window: &Self, _, parameter| {
                if let Some(section) = parameter.and_then(|p| p.get::<String>()) {
                    window.select_section(&section);
                }
            })
            .build();
        let search = gio::ActionEntry::builder("search")
            .activate(|window: &Self, _, _| {
                window.select_section("browse");
                window.imp().browse_page.focus_search();
            })
            .build();
        self.add_action_entries([go, search]);
    }

    /// Select the sidebar row, which shows the section.
    fn select_section(&self, section: &str) {
        let imp = self.imp();
        if let Some(index) = SECTIONS.iter().position(|s| *s == section)
            && let Some(row) = imp.sidebar_list.row_at_index(index as i32)
        {
            imp.sidebar_list.select_row(Some(&row));
        }
    }

    fn show_section(&self, section: &str) {
        let imp = self.imp();
        imp.content_nav.pop_to_tag("root");
        imp.section_stack.set_visible_child_name(section);
        imp.section_title.set_title(&match section {
            "discover" => gettext("Discover"),
            "timeline" => gettext("Timeline"),
            _ => gettext("Browse"),
        });
        imp.split.set_show_content(true);
        if section == "timeline" {
            imp.timeline_page.ensure_loaded();
        }
    }

    /// The catalog was replaced: every section re-reads it.
    pub fn catalog_changed(&self) {
        let imp = self.imp();
        let db = self.db();
        let (count, generated) = {
            let db = db.borrow();
            (
                db.count().unwrap_or(0),
                db.meta("index-generated-at").ok().flatten(),
            )
        };
        let when = generated
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok())
            .map(|at| {
                at.with_timezone(&chrono::Local)
                    .format("%-d %b %Y")
                    .to_string()
            });
        imp.stats_label.set_text(&match (count, when) {
            (0, _) => gettext("No catalog yet"),
            (n, Some(when)) => format(
                &plural(
                    "{n} project, indexed {when}",
                    "{n} projects, indexed {when}",
                    n,
                ),
                &[("n", &n.to_string()), ("when", &when)],
            ),
            (n, None) => format(
                &plural("{n} project", "{n} projects", n),
                &[("n", &n.to_string())],
            ),
        });
        imp.browse_page.reload();
        imp.timeline_page.reload();
    }

    pub fn set_refreshing(&self, refreshing: bool) {
        self.imp().refresh_spinner.set_visible(refreshing);
    }

    pub fn toast(&self, message: &str) {
        tracing::info!("{message}");
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }

    /// Push the project's page over the current section.
    pub fn open_entry(&self, entry: &Entry) {
        tracing::debug!("opening {}", entry.id);
        let page = ProjectPage::new(self, entry);
        self.imp().content_nav.push(&page);
    }

    /// Push a page listing everything by one developer.
    pub fn open_author(&self, author_key: &str, author: &str) {
        let page = author::page(self, author_key, author);
        self.imp().content_nav.push(&page);
    }

    /// Open a link in the browser.
    pub fn open_uri(&self, uri: &str) {
        if uri.is_empty() {
            return;
        }
        gtk::UriLauncher::new(uri).launch(Some(self), gio::Cancellable::NONE, |result| {
            if let Err(error) = result {
                tracing::warn!("could not open the link: {error}");
            }
        });
    }

    fn save_geometry(&self) {
        let settings = self.settings();
        let (width, height) = self.default_size();
        let _ = settings.set_int(keys::WINDOW_WIDTH, width);
        let _ = settings.set_int(keys::WINDOW_HEIGHT, height);
        let _ = settings.set_boolean(keys::WINDOW_MAXIMIZED, self.is_maximized());
    }
}
