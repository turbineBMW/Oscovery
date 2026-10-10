//! Timeline: what entered the catalog, day by day, newest first. Days load
//! a few at a time as the user scrolls down; a day with many entries shows
//! a first batch and a button for the rest.

use super::OscoveryWindow;
use crate::i18n::{format, gettext, plural};
use crate::widgets;
use chrono::NaiveDate;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::{Cell, RefCell};

const DAYS_PER_LOAD: usize = 5;
const CARDS_PER_DAY: usize = 24;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/turbinebmw/Oscovery/ui/timeline-page.ui")]
    pub struct TimelinePage {
        #[template_child]
        pub scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub days_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub earlier_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub empty_page: TemplateChild<adw::StatusPage>,

        pub window: glib::WeakRef<OscoveryWindow>,
        /// The oldest day shown; the next load starts before it.
        pub oldest: RefCell<Option<NaiveDate>>,
        pub loaded: Cell<bool>,
        pub exhausted: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TimelinePage {
        const NAME: &'static str = "OscoveryTimelinePage";
        type Type = super::TimelinePage;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for TimelinePage {}
    impl WidgetImpl for TimelinePage {}
    impl BoxImpl for TimelinePage {}
}

glib::wrapper! {
    pub struct TimelinePage(ObjectSubclass<imp::TimelinePage>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl TimelinePage {
    pub fn setup(&self, window: &OscoveryWindow) {
        let imp = self.imp();
        imp.window.set(Some(window));
        imp.earlier_button.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.load_more()
        ));
        imp.scroller.connect_edge_reached(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_, position| {
                if position == gtk::PositionType::Bottom {
                    page.load_more();
                }
            }
        ));
    }

    /// The catalog changed: start over, but only build cards if the page
    /// has been shown (it is the heaviest one).
    pub fn reload(&self) {
        let imp = self.imp();
        let was_loaded = imp.loaded.replace(false);
        imp.oldest.replace(None);
        imp.exhausted.set(false);
        while let Some(child) = imp.days_box.first_child() {
            imp.days_box.remove(&child);
        }
        if was_loaded || self.is_mapped() {
            self.ensure_loaded();
        }
    }

    pub fn ensure_loaded(&self) {
        if !self.imp().loaded.replace(true) {
            self.load_more();
        }
    }

    fn load_more(&self) {
        let imp = self.imp();
        if imp.exhausted.get() {
            return;
        }
        let Some(window) = imp.window.upgrade() else {
            return;
        };
        let db = window.db();
        let before = *imp.oldest.borrow();
        let days = db
            .borrow()
            .timeline_days(before, DAYS_PER_LOAD)
            .unwrap_or_default();
        if days.len() < DAYS_PER_LOAD {
            imp.exhausted.set(true);
        }
        for (day, count) in &days {
            let entries = db.borrow().entries_on(*day).unwrap_or_default();
            self.append_day(&window, *day, *count, entries);
            imp.oldest.replace(Some(*day));
        }
        let nothing = imp.days_box.first_child().is_none();
        imp.empty_page.set_visible(nothing);
        imp.earlier_button
            .set_visible(!nothing && !imp.exhausted.get());
    }

    fn append_day(
        &self,
        window: &OscoveryWindow,
        day: NaiveDate,
        count: usize,
        entries: Vec<oscovery_core::models::Entry>,
    ) {
        let imp = self.imp();
        let header = gtk::Box::builder()
            .spacing(8)
            .css_classes(["day-header"])
            .build();
        let date = gtk::Label::builder()
            .label(day_label(day))
            .xalign(0.0)
            .css_classes(["title-3"])
            .build();
        let number = gtk::Label::builder()
            .label(format(
                &plural("{n} new", "{n} new", count),
                &[("n", &count.to_string())],
            ))
            .valign(gtk::Align::Baseline)
            .css_classes(["dim-label"])
            .build();
        header.append(&date);
        header.append(&number);
        imp.days_box.append(&header);

        let flow = gtk::FlowBox::builder()
            .homogeneous(true)
            .selection_mode(gtk::SelectionMode::None)
            .min_children_per_line(1)
            .max_children_per_line(8)
            .column_spacing(12)
            .row_spacing(12)
            .valign(gtk::Align::Start)
            .build();
        widgets::open_on_activate(&flow, window);
        let (first, rest) = entries.split_at(entries.len().min(CARDS_PER_DAY));
        widgets::fill_cards(&flow, first);
        imp.days_box.append(&flow);
        if !rest.is_empty() {
            let rest = rest.to_vec();
            let button = gtk::Button::builder()
                .label(format(
                    &gettext("Show {n} more from this day"),
                    &[("n", &rest.len().to_string())],
                ))
                .halign(gtk::Align::Start)
                .css_classes(["flat"])
                .build();
            button.connect_clicked(glib::clone!(
                #[weak]
                flow,
                move |button| {
                    widgets::append_cards(&flow, &rest);
                    button.set_visible(false);
                }
            ));
            imp.days_box.append(&button);
        }
    }
}

/// "Today", "Yesterday", or the date in words.
fn day_label(day: NaiveDate) -> String {
    let today = chrono::Local::now().date_naive();
    if day == today {
        gettext("Today")
    } else if today.pred_opt() == Some(day) {
        gettext("Yesterday")
    } else if today.signed_duration_since(day).num_days() < 300 {
        day.format("%A, %-d %B").to_string()
    } else {
        day.format("%-d %B %Y").to_string()
    }
}
