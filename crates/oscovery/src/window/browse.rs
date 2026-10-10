//! Browse: a search box, kind, category and sort filters, and a grid of
//! cards paged by "Show more".

use super::OscoveryWindow;
use crate::i18n::{format, gettext, plural};
use crate::widgets;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use oscovery_core::db::{Query, Sort};
use oscovery_core::models::Kind;
use oscovery_core::retrieve::{fts_any, fts_match};
use std::cell::{Cell, RefCell};
use std::time::Duration;

const PAGE: usize = 48;
const DEBOUNCE: Duration = Duration::from_millis(150);

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/turbinebmw/Oscovery/ui/browse-page.ui")]
    pub struct BrowsePage {
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub kind_dropdown: TemplateChild<gtk::DropDown>,
        #[template_child]
        pub category_dropdown: TemplateChild<gtk::DropDown>,
        #[template_child]
        pub sort_dropdown: TemplateChild<gtk::DropDown>,
        #[template_child]
        pub scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub result_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub results: TemplateChild<gtk::FlowBox>,
        #[template_child]
        pub more_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub empty_page: TemplateChild<adw::StatusPage>,

        pub window: glib::WeakRef<OscoveryWindow>,
        /// Category names behind the dropdown, after "All categories".
        pub categories: RefCell<Vec<String>>,
        /// How many results are shown; the next page starts here.
        pub shown: Cell<usize>,
        /// Bumps on every edit so a stale debounce does nothing.
        pub generation: Cell<u64>,
        /// Rebuilding the category list must not trigger a search.
        pub quiet: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BrowsePage {
        const NAME: &'static str = "OscoveryBrowsePage";
        type Type = super::BrowsePage;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for BrowsePage {}
    impl WidgetImpl for BrowsePage {}
    impl BoxImpl for BrowsePage {}
}

glib::wrapper! {
    pub struct BrowsePage(ObjectSubclass<imp::BrowsePage>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl BrowsePage {
    pub fn setup(&self, window: &OscoveryWindow) {
        let imp = self.imp();
        imp.window.set(Some(window));
        imp.kind_dropdown.set_model(Some(&gtk::StringList::new(&[
            &gettext("All kinds"),
            &gettext("Plugins"),
            &gettext("Apps"),
        ])));
        imp.sort_dropdown.set_model(Some(&gtk::StringList::new(&[
            &gettext("Best match"),
            &gettext("Most starred"),
            &gettext("Newest"),
            &gettext("Recently updated"),
            &gettext("Name"),
        ])));
        imp.category_dropdown
            .set_model(Some(&gtk::StringList::new(&[&gettext("All categories")])));
        widgets::open_on_activate(&imp.results, window);

        imp.search_entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.search_soon()
        ));
        for dropdown in [
            &imp.kind_dropdown,
            &imp.category_dropdown,
            &imp.sort_dropdown,
        ] {
            dropdown.connect_selected_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_| {
                    if !page.imp().quiet.get() {
                        page.search_now();
                    }
                }
            ));
        }
        imp.more_button.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.show_more()
        ));
    }

    pub fn focus_search(&self) {
        self.imp().search_entry.grab_focus();
    }

    /// The catalog changed: new categories, same filters, fresh results.
    pub fn reload(&self) {
        let imp = self.imp();
        let Some(window) = imp.window.upgrade() else {
            return;
        };
        let categories: Vec<String> = window
            .db()
            .borrow()
            .categories(None)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let selected = imp.category_dropdown.selected() as usize;
        let selected_name = (selected > 0)
            .then(|| imp.categories.borrow().get(selected - 1).cloned())
            .flatten();
        let mut names: Vec<&str> = vec![];
        let all = gettext("All categories");
        names.push(&all);
        names.extend(categories.iter().map(String::as_str));
        imp.quiet.set(true);
        imp.category_dropdown
            .set_model(Some(&gtk::StringList::new(&names)));
        if let Some(name) = selected_name
            && let Some(index) = categories.iter().position(|c| *c == name)
        {
            imp.category_dropdown.set_selected(index as u32 + 1);
        }
        imp.quiet.set(false);
        imp.categories.replace(categories);
        self.search_now();
    }

    fn search_soon(&self) {
        let imp = self.imp();
        let generation = imp.generation.get() + 1;
        imp.generation.set(generation);
        glib::timeout_add_local_once(
            DEBOUNCE,
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                move || {
                    if page.imp().generation.get() == generation {
                        page.search_now();
                    }
                }
            ),
        );
    }

    fn query(&self, offset: usize) -> Query {
        let imp = self.imp();
        let text = imp.search_entry.text().to_string();
        let kind = match imp.kind_dropdown.selected() {
            1 => Some(Kind::Plugin),
            2 => Some(Kind::App),
            _ => None,
        };
        let category = match imp.category_dropdown.selected() as usize {
            0 => None,
            index => imp.categories.borrow().get(index - 1).cloned(),
        };
        let sort = match imp.sort_dropdown.selected() {
            1 => Sort::Stars,
            2 => Sort::Newest,
            3 => Sort::Updated,
            4 => Sort::Name,
            _ => Sort::Relevance,
        };
        Query {
            fts: fts_match(&text),
            kind,
            category,
            sort,
            limit: PAGE,
            offset,
        }
    }

    fn search_now(&self) {
        let imp = self.imp();
        let Some(window) = imp.window.upgrade() else {
            return;
        };
        let mut query = self.query(0);
        let db = window.db();
        let mut entries = db.borrow().search(&query).unwrap_or_default();
        // A sentence with no exact matches: let any of its words count.
        if entries.is_empty()
            && let Some(any) = fts_any(&imp.search_entry.text())
            && any.contains(" OR ")
        {
            query.fts = Some(any);
            entries = db.borrow().search(&query).unwrap_or_default();
        }
        imp.shown.set(entries.len());
        widgets::fill_cards(&imp.results, &entries);
        imp.more_button.set_visible(entries.len() == PAGE);
        imp.empty_page.set_visible(entries.is_empty());
        imp.results.set_visible(!entries.is_empty());
        imp.result_label.set_text(&if entries.is_empty() {
            String::new()
        } else if entries.len() == PAGE {
            format(
                &gettext("Showing the first {n}"),
                &[("n", &entries.len().to_string())],
            )
        } else {
            format(
                &plural("{n} project", "{n} projects", entries.len()),
                &[("n", &entries.len().to_string())],
            )
        });
        imp.scroller.vadjustment().set_value(0.0);
    }

    fn show_more(&self) {
        let imp = self.imp();
        let Some(window) = imp.window.upgrade() else {
            return;
        };
        let query = self.query(imp.shown.get());
        let entries = window.db().borrow().search(&query).unwrap_or_default();
        imp.shown.set(imp.shown.get() + entries.len());
        widgets::append_cards(&imp.results, &entries);
        imp.more_button.set_visible(entries.len() == PAGE);
        imp.result_label.set_text(&format(
            &gettext("Showing the first {n}"),
            &[("n", &imp.shown.get().to_string())],
        ));
    }
}
