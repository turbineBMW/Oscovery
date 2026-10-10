//! A project's page: what it is, how to install it, more by the same
//! developer, and similar projects.

use super::OscoveryWindow;
use crate::i18n::{format, gettext};
use crate::widgets::entry_card::kind_label;
use crate::{images, widgets};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use oscovery_core::db::Query;
use oscovery_core::models::{Entry, InstallMethod};
use oscovery_core::retrieve::fts_any;
use std::cell::RefCell;

const MORE_BY_LIMIT: usize = 12;
const SIMILAR_LIMIT: usize = 8;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/turbinebmw/Oscovery/ui/project-page.ui")]
    pub struct ProjectPage {
        #[template_child]
        pub initials_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub name_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub author_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub version_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub chips_box: TemplateChild<gtk::FlowBox>,
        #[template_child]
        pub install_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub review_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub repo_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub listing_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub install_command: TemplateChild<gtk::Label>,
        #[template_child]
        pub install_note: TemplateChild<gtk::Label>,
        #[template_child]
        pub preview: TemplateChild<gtk::Picture>,
        #[template_child]
        pub summary_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub description_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub tags_box: TemplateChild<gtk::FlowBox>,
        #[template_child]
        pub meta_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub more_group: TemplateChild<gtk::Box>,
        #[template_child]
        pub more_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub more_box: TemplateChild<gtk::FlowBox>,
        #[template_child]
        pub similar_group: TemplateChild<gtk::Box>,
        #[template_child]
        pub similar_box: TemplateChild<gtk::FlowBox>,

        pub window: glib::WeakRef<OscoveryWindow>,
        pub entry: RefCell<Option<Entry>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ProjectPage {
        const NAME: &'static str = "OscoveryProjectPage";
        type Type = super::ProjectPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ProjectPage {}
    impl WidgetImpl for ProjectPage {}
    impl NavigationPageImpl for ProjectPage {}
}

glib::wrapper! {
    pub struct ProjectPage(ObjectSubclass<imp::ProjectPage>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ProjectPage {
    pub fn new(window: &OscoveryWindow, entry: &Entry) -> Self {
        let page: Self = glib::Object::new();
        let imp = page.imp();
        imp.window.set(Some(window));
        imp.entry.replace(Some(entry.clone()));
        NavigationPageExt::set_title(&page, &entry.name);
        page.fill(window, entry);
        page
    }

    fn fill(&self, window: &OscoveryWindow, entry: &Entry) {
        let imp = self.imp();
        imp.initials_label.set_text(&widgets::initials(&entry.name));
        imp.name_label.set_text(&entry.name);
        imp.author_button.set_label(&if entry.author.is_empty() {
            entry.author_key.clone()
        } else {
            entry.author.clone()
        });
        imp.author_button
            .set_visible(!entry.author_key.is_empty() || !entry.author.is_empty());
        imp.version_label.set_text(&entry.version);
        imp.version_label.set_visible(!entry.version.is_empty());

        imp.chips_box
            .append(&widgets::chip(&kind_label(entry), "kind"));
        if !entry.category.is_empty() {
            imp.chips_box.append(&widgets::chip(&entry.category, ""));
        }
        if entry.verified {
            imp.chips_box
                .append(&widgets::chip(&gettext("Verified listing"), "verified"));
        }
        if !entry.license.is_empty() {
            imp.chips_box
                .append(&widgets::chip(&entry.license, "muted"));
        }
        if entry.wants_contributors {
            imp.chips_box
                .append(&widgets::chip(&gettext("Wants contributors"), ""));
        }
        for tag in &entry.tags {
            imp.tags_box
                .append(&widgets::chip(&format!("#{tag}"), "muted"));
        }
        imp.tags_box.set_visible(!entry.tags.is_empty());

        imp.summary_label.set_text(&entry.summary);
        imp.summary_label.set_visible(!entry.summary.is_empty());
        imp.description_label.set_text(&entry.description);
        imp.description_label
            .set_visible(!entry.description.is_empty());

        let mut meta = Vec::new();
        if entry.stars > 0 {
            meta.push(format(
                &gettext("{n} stars"),
                &[("n", &entry.stars.to_string())],
            ));
        }
        if let Some(updated) = entry.updated_at {
            meta.push(format(
                &gettext("updated {date}"),
                &[("date", &updated.format("%-d %b %Y").to_string())],
            ));
        }
        meta.push(format(
            &gettext("in the catalog since {date}"),
            &[("date", &entry.first_seen.format("%-d %b %Y").to_string())],
        ));
        if !entry.status.is_empty() && entry.status != "Available" {
            meta.push(entry.status.clone());
        }
        imp.meta_label.set_text(&meta.join(" · "));

        // Install: the command is shown before it ever runs. Running it is
        // the next phase; until then the button explains itself.
        match &entry.install {
            Some(install) if !install.command.is_empty() => {
                imp.install_command.set_text(&install.command);
                imp.install_command.set_visible(true);
                imp.install_button.set_sensitive(false);
                imp.install_button.set_tooltip_text(Some(&gettext(
                    "Installing from here is coming next; the command above works today.",
                )));
                if install.method == InstallMethod::OmarchyBuiltin {
                    imp.install_button.set_label(&gettext("Enable"));
                }
            }
            Some(install) => {
                imp.install_button.set_sensitive(false);
                imp.install_button
                    .set_tooltip_text(Some(&gettext("No one-step install; see the repository.")));
                imp.install_note.set_text(&install.note);
            }
            None => {
                imp.install_button.set_sensitive(false);
                imp.install_button.set_visible(false);
            }
        }
        if let Some(install) = &entry.install
            && !install.note.is_empty()
            && install.method != InstallMethod::OmarchyPlugin
        {
            imp.install_note.set_text(&install.note);
            imp.install_note.set_visible(true);
        }
        imp.review_button.set_sensitive(false);
        imp.review_button.set_tooltip_text(Some(&gettext(
            "Your coding agent reads the source and reports before anything installs. Coming next.",
        )));

        let repo = entry.repo.clone();
        imp.repo_button.set_visible(!repo.is_empty());
        imp.repo_button.connect_clicked(glib::clone!(
            #[weak]
            window,
            move |_| window.open_uri(&repo)
        ));
        let listing = entry.listing_url.clone();
        imp.listing_button.set_visible(!listing.is_empty());
        imp.listing_button.connect_clicked(glib::clone!(
            #[weak]
            window,
            move |_| window.open_uri(&listing)
        ));
        let (author_key, author) = (entry.author_key.clone(), entry.author.clone());
        imp.author_button.connect_clicked(glib::clone!(
            #[weak]
            window,
            move |_| window.open_author(&author_key, &author)
        ));

        if let Some(url) = entry.previews.first() {
            let preview = imp.preview.clone();
            images::load_into(&imp.preview, url, move || preview.set_visible(true));
        }

        // More by this developer.
        let db = window.db();
        let more = db
            .borrow()
            .by_author(&entry.author_key, &entry.id)
            .unwrap_or_default();
        if !more.is_empty() {
            imp.more_title.set_text(&format(
                &gettext("More by {author}"),
                &[("author", &imp.author_button.label().unwrap_or_default())],
            ));
            widgets::open_on_activate(&imp.more_box, window);
            widgets::fill_cards(&imp.more_box, &more[..more.len().min(MORE_BY_LIMIT)]);
            imp.more_group.set_visible(true);
        }

        // Similar: the same words, tags and category, minus itself.
        let words = format!(
            "{} {} {}",
            entry.tags.join(" "),
            entry.category,
            entry.summary
        );
        if let Some(fts) = fts_any(&words) {
            let similar: Vec<Entry> = db
                .borrow()
                .search(&Query {
                    fts: Some(fts),
                    kind: Some(entry.kind),
                    limit: SIMILAR_LIMIT + 1,
                    ..Default::default()
                })
                .unwrap_or_default()
                .into_iter()
                .filter(|other| other.id != entry.id)
                .take(SIMILAR_LIMIT)
                .collect();
            if !similar.is_empty() {
                widgets::open_on_activate(&imp.similar_box, window);
                widgets::fill_cards(&imp.similar_box, &similar);
                imp.similar_group.set_visible(true);
            }
        }
    }
}
