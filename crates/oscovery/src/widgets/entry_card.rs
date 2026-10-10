//! One catalog entry as a card: thumbnail or initials, name, author,
//! summary, category and stars.

use crate::{images, widgets};
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use oscovery_core::models::{Entry, Kind};
use std::cell::RefCell;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/turbinebmw/Oscovery/ui/entry-card.ui")]
    pub struct EntryCard {
        #[template_child]
        pub thumb_overlay: TemplateChild<gtk::Overlay>,
        #[template_child]
        pub initials_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub thumbnail: TemplateChild<gtk::Picture>,
        #[template_child]
        pub name_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub kind_chip: TemplateChild<gtk::Label>,
        #[template_child]
        pub author_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub summary_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub category_chip: TemplateChild<gtk::Label>,
        #[template_child]
        pub stars_label: TemplateChild<gtk::Label>,
        pub entry: RefCell<Option<Entry>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EntryCard {
        const NAME: &'static str = "OscoveryEntryCard";
        type Type = super::EntryCard;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for EntryCard {
        fn constructed(&self) {
            self.parent_constructed();
            // The picture is drawn over the placeholder and never sizes the
            // card: a wide preview must not widen the grid's columns.
            self.thumb_overlay.set_clip_overlay(&*self.thumbnail, true);
            self.thumb_overlay
                .set_measure_overlay(&*self.thumbnail, false);
        }
    }
    impl WidgetImpl for EntryCard {}
    impl BoxImpl for EntryCard {}
}

glib::wrapper! {
    pub struct EntryCard(ObjectSubclass<imp::EntryCard>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl EntryCard {
    pub fn new(entry: &Entry) -> Self {
        let card: Self = glib::Object::new();
        card.set_entry(entry);
        card
    }

    pub fn entry(&self) -> Entry {
        self.imp()
            .entry
            .borrow()
            .clone()
            .expect("a card always shows an entry")
    }

    fn set_entry(&self, entry: &Entry) {
        let imp = self.imp();
        imp.name_label.set_text(&entry.name);
        imp.kind_chip.set_text(&kind_label(entry));
        imp.author_label.set_text(&entry.author);
        imp.author_label.set_visible(!entry.author.is_empty());
        imp.summary_label.set_text(&entry.summary);
        imp.category_chip.set_text(&entry.category);
        imp.category_chip.set_visible(!entry.category.is_empty());
        imp.stars_label
            .set_text(&widgets::compact_count(entry.stars));
        imp.initials_label.set_text(&widgets::initials(&entry.name));
        imp.thumbnail.set_visible(false);
        self.set_tooltip_text(
            Some(&entry.summary)
                .filter(|s| !s.is_empty())
                .map(String::as_str),
        );
        if !entry.thumbnail.is_empty() {
            let picture = imp.thumbnail.clone();
            images::load_into(&imp.thumbnail, &entry.thumbnail, move || {
                picture.set_visible(true);
            });
        }
        imp.entry.replace(Some(entry.clone()));
    }
}

/// The short word on a card: a plugin's subkind ("Bar widget"), an app's
/// source ("AUR").
pub fn kind_label(entry: &Entry) -> String {
    match entry.kind {
        Kind::Plugin => match entry.subkind.as_str() {
            "" | "plugin" => "Plugin".into(),
            subkind => {
                let text = subkind.replace('-', " ");
                let mut chars = text.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => text,
                }
            }
        },
        Kind::App => match entry.subkind.as_str() {
            "aur" => "AUR".into(),
            "github-release" => "Release".into(),
            _ => "App".into(),
        },
    }
}
