//! A project's page: what it is, how to install it, its README as GitHub
//! shows it, more by the same developer, and similar projects.

use super::OscoveryWindow;
use crate::i18n::{format, gettext};
use crate::rt;
use crate::widgets::entry_card::kind_label;
use crate::widgets::markdown::{self as readme_view, OnLink};
use crate::{config, images, widgets};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use oscovery_core::db::Query;
use oscovery_core::install::{self, Action};
use oscovery_core::markdown;
use oscovery_core::models::{Entry, InstallMethod};
use oscovery_core::readme::{self, Readme};
use oscovery_core::retrieve::fts_any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

const MORE_BY_LIMIT: usize = 12;
const SIMILAR_LIMIT: usize = 8;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/turbinebmw/Oscovery/ui/project-page.ui")]
    pub struct ProjectPage {
        #[template_child]
        pub scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub initials_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub name_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub author_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub version_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub stars_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub stars_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub updated_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub updated_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub slug_label: TemplateChild<gtk::Label>,
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
        pub manage_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub installed_chip: TemplateChild<gtk::Label>,
        #[template_child]
        pub enable_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub disable_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub remove_button: TemplateChild<gtk::Button>,
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
        pub readme_card: TemplateChild<gtk::Box>,
        #[template_child]
        pub readme_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub readme_spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        pub readme_open_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub readme_body: TemplateChild<adw::Bin>,
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
        /// The README's headings, for its `#links`.
        pub anchors: RefCell<HashMap<String, gtk::Widget>>,
        /// The README shows the project itself, so the listing's preview
        /// would only repeat it.
        pub readme_has_pictures: Cell<bool>,
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

    /// Ask, showing the exact command, then run it and report.
    fn confirm_and_run(&self, action: &Action, verb: &str) {
        let imp = self.imp();
        let Some(window) = imp.window.upgrade() else {
            return;
        };
        let name = imp.name_label.text().to_string();
        let dialog = adw::AlertDialog::builder()
            .heading(format(
                &gettext("{verb} {name}?"),
                &[("verb", verb), ("name", &name)],
            ))
            .body(action.display())
            .build();
        dialog.add_responses(&[("cancel", &gettext("Cancel")), ("go", verb)]);
        dialog.set_response_appearance(
            "go",
            if verb == gettext("Remove") {
                adw::ResponseAppearance::Destructive
            } else {
                adw::ResponseAppearance::Suggested
            },
        );
        dialog.set_default_response(Some("go"));
        let action = action.clone();
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                #[weak]
                window,
                move |_, response| {
                    if response != "go" {
                        return;
                    }
                    let imp = page.imp();
                    for button in [
                        &*imp.install_button,
                        &*imp.enable_button,
                        &*imp.disable_button,
                        &*imp.remove_button,
                    ] {
                        button.set_sensitive(false);
                    }
                    let action = action.clone();
                    let shown = action.display();
                    rt::run(
                        async move { install::run(&action).await },
                        glib::clone!(
                            #[weak]
                            page,
                            #[weak]
                            window,
                            move |result| {
                                page.imp().install_button.set_sensitive(true);
                                match result {
                                    Ok(outcome) if outcome.success => {
                                        tracing::info!("ran: {shown}");
                                        window.toast(&gettext("Done."));
                                    }
                                    Ok(outcome) => {
                                        tracing::warn!(
                                            "failed: {shown}\n{}\n{}",
                                            outcome.stdout,
                                            outcome.stderr
                                        );
                                        window.toast(&outcome.complaint());
                                    }
                                    Err(error) => {
                                        tracing::warn!("failed: {shown}: {error}");
                                        window.toast(&error.to_string());
                                    }
                                }
                                page.refresh_installed();
                            }
                        ),
                    );
                }
            ),
        );
        dialog.present(Some(&window));
    }

    /// Read back whether this plugin is installed and enabled, and show
    /// the right buttons.
    fn refresh_installed(&self) {
        let imp = self.imp();
        let Some(entry) = imp.entry.borrow().clone() else {
            return;
        };
        if !entry.is_plugin() {
            return;
        }
        let id = entry.id.clone();
        rt::run(
            async move { install::installed_plugins().await.remove(&id) },
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |installed| {
                    let imp = page.imp();
                    let Some(plugin) = installed else {
                        imp.manage_box.set_visible(false);
                        imp.install_button.set_visible(entry.install.is_some());
                        return;
                    };
                    imp.manage_box.set_visible(true);
                    imp.install_button.set_visible(false);
                    imp.installed_chip.set_text(&if plugin.enabled {
                        gettext("Installed and enabled")
                    } else {
                        gettext("Installed, not enabled")
                    });
                    imp.enable_button.set_visible(!plugin.enabled);
                    imp.enable_button.set_sensitive(true);
                    imp.disable_button
                        .set_visible(plugin.enabled && plugin.can_disable);
                    imp.disable_button.set_sensitive(true);
                    imp.remove_button.set_visible(!plugin.first_party);
                    imp.remove_button.set_sensitive(true);
                }
            ),
        );
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

        imp.stars_label
            .set_text(&widgets::compact_count(entry.stars));
        imp.stars_box.set_visible(entry.stars > 0);
        if let Some(updated) = entry.updated_at {
            imp.updated_label.set_text(&format(
                &gettext("updated {date}"),
                &[("date", &updated.format("%-d %b %Y").to_string())],
            ));
            imp.updated_box.set_visible(true);
        }
        if let Some(slug) = entry.github_slug() {
            imp.slug_label.set_markup(&format!(
                "<a href=\"{}\">{}</a>",
                markdown::escape(&entry.repo),
                markdown::escape(&slug)
            ));
            imp.slug_label.set_tooltip_text(Some(&entry.repo));
            imp.slug_label.set_visible(true);
            imp.slug_label.connect_activate_link(glib::clone!(
                #[weak]
                window,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, uri| {
                    window.open_uri(uri);
                    glib::Propagation::Stop
                }
            ));
            self.load_readme(slug);
        }

        let mut meta = Vec::new();
        meta.push(format(
            &gettext("in the catalog since {date}"),
            &[("date", &entry.first_seen.format("%-d %b %Y").to_string())],
        ));
        if !entry.status.is_empty() && entry.status != "Available" {
            meta.push(entry.status.clone());
        }
        imp.meta_label.set_text(&meta.join(" · "));

        // Install: the command is shown before it ever runs, and again in
        // the confirmation.
        let action = install::install_action(entry);
        match (&action, &entry.install) {
            (Action::Manual, Some(install)) => {
                imp.install_button.set_sensitive(false);
                imp.install_button
                    .set_tooltip_text(Some(&gettext("No one-step install; see the repository.")));
                if !install.note.is_empty() {
                    imp.install_note.set_text(&install.note);
                    imp.install_note.set_visible(true);
                }
            }
            (Action::Manual, None) => imp.install_button.set_visible(false),
            (action, install) => {
                imp.install_command.set_text(&action.display());
                imp.install_command.set_visible(true);
                if let Some(install) = install
                    && install.method == InstallMethod::OmarchyBuiltin
                {
                    imp.install_button.set_label(&gettext("Enable"));
                }
                if matches!(action, Action::Terminal { .. }) {
                    imp.install_note.set_text(&gettext(
                        "Runs in a terminal, where you can watch and answer prompts.",
                    ));
                    imp.install_note.set_visible(true);
                }
                let action = action.clone();
                imp.install_button.connect_clicked(glib::clone!(
                    #[weak(rename_to = page)]
                    self,
                    move |_| page.confirm_and_run(&action, &gettext("Install"))
                ));
            }
        }
        for (button, action) in [
            (&imp.enable_button, install::enable_action(&entry.id)),
            (&imp.disable_button, install::disable_action(&entry.id)),
            (&imp.remove_button, install::remove_action(&entry.id)),
        ] {
            let label = button.label().unwrap_or_default().to_string();
            button.connect_clicked(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_| page.confirm_and_run(&action, &label)
            ));
        }
        self.refresh_installed();
        let reviewable =
            oscovery_core::review::source_url(entry).is_some() && !matches!(action, Action::Manual);
        imp.review_button.set_visible(reviewable);
        let for_review = entry.clone();
        imp.review_button.connect_clicked(glib::clone!(
            #[weak]
            window,
            move |_| window.open_review(&for_review)
        ));

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
            images::load_into(
                &imp.preview,
                url,
                glib::clone!(
                    #[weak(rename_to = page)]
                    self,
                    move || {
                        let imp = page.imp();
                        imp.preview.set_visible(!imp.readme_has_pictures.get());
                    }
                ),
            );
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

    /// Fetch the README (or its cached copy) and show it under the header;
    /// the index's excerpt stays when there is none.
    fn load_readme(&self, slug: String) {
        let imp = self.imp();
        imp.readme_card.set_visible(true);
        let cache = glib::user_cache_dir()
            .join(config::DIR_NAME)
            .join("readmes");
        rt::run(
            async move { readme::fetch(&slug, &cache).await },
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |result| match result {
                    Ok(Some(readme)) => page.show_readme(readme),
                    Ok(None) => page.imp().readme_card.set_visible(false),
                    Err(error) => {
                        tracing::info!("readme: {error}");
                        let imp = page.imp();
                        imp.readme_spinner.set_visible(false);
                        let label = gtk::Label::builder()
                            .label(format(
                                &gettext("The README could not be loaded: {error}"),
                                &[("error", &error.to_string())],
                            ))
                            .xalign(0.0)
                            .wrap(true)
                            .build();
                        label.add_css_class("dim-label");
                        imp.readme_body.set_child(Some(&label));
                    }
                }
            ),
        );
    }

    fn show_readme(&self, readme: Readme) {
        let imp = self.imp();
        imp.readme_spinner.set_visible(false);
        imp.readme_name.set_text(&readme.name);
        let html_url = readme.html_url.clone();
        imp.readme_open_button.set_visible(true);
        imp.readme_open_button.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.follow(&html_url)
        ));
        if readme.is_markdown() {
            let blocks = markdown::parse(&readme.text, &readme.base);
            let on_link: OnLink = Rc::new(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |uri: &str| page.follow(uri)
            ));
            let rendered = readme_view::render(&blocks, on_link);
            imp.readme_body.set_child(Some(&rendered.widget));
            imp.anchors.replace(rendered.anchors);
            if markdown::has_pictures(&blocks) {
                imp.readme_has_pictures.set(true);
                imp.preview.set_visible(false);
            }
        } else {
            imp.readme_body
                .set_child(Some(&readme_view::plain_text(&readme.text)));
        }
        // The README is what the excerpt was cut from.
        imp.description_label.set_visible(false);
    }

    /// A link in the README: its own `#headings` scroll the page, the rest
    /// open in the browser.
    fn follow(&self, uri: &str) {
        let imp = self.imp();
        if let Some(anchor) = uri.strip_prefix('#') {
            let anchors = imp.anchors.borrow();
            let Some(heading) = anchors.get(&anchor.to_lowercase()) else {
                return;
            };
            let adjustment = imp.scroller.vadjustment();
            if let Some(point) =
                heading.compute_point(&*imp.scroller, &gtk::graphene::Point::new(0.0, 0.0))
            {
                adjustment.set_value(adjustment.value() + point.y() as f64 - 12.0);
            }
        } else if let Some(window) = imp.window.upgrade() {
            window.open_uri(uri);
        }
    }
}
