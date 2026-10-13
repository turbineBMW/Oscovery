pub mod entry_card;
pub mod markdown;

use crate::window::OscoveryWindow;
use entry_card::EntryCard;
use gtk::glib;
use gtk::prelude::*;
use oscovery_core::models::Entry;

/// A chip label: a kind, category or tag.
pub fn chip(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("chip");
    if !class.is_empty() {
        label.add_css_class(class);
    }
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(28);
    label
}

/// Fill a flow box with cards for `entries`, replacing what was there.
pub fn fill_cards(flow_box: &gtk::FlowBox, entries: &[Entry]) {
    flow_box.remove_all();
    append_cards(flow_box, entries);
}

pub fn append_cards(flow_box: &gtk::FlowBox, entries: &[Entry]) {
    for entry in entries {
        let card = EntryCard::new(entry);
        let child = gtk::FlowBoxChild::builder().child(&card).build();
        child.add_css_class("card-child");
        flow_box.append(&child);
    }
}

/// Opening a card opens its project. Call once per flow box.
pub fn open_on_activate(flow_box: &gtk::FlowBox, window: &OscoveryWindow) {
    flow_box.connect_child_activated(glib::clone!(
        #[weak]
        window,
        move |_, child| {
            if let Some(card) = child.child().and_downcast::<EntryCard>() {
                window.open_entry(&card.entry());
            }
        }
    ));
}

/// Initials for a project without a picture: up to two letters.
pub fn initials(name: &str) -> String {
    let mut words = name
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty());
    let first = words.next().and_then(|w| w.chars().next());
    let second = words.next().and_then(|w| w.chars().next());
    match (first, second) {
        (Some(a), Some(b)) => format!("{}{}", a.to_uppercase(), b.to_uppercase()),
        (Some(a), None) => a.to_uppercase().to_string(),
        _ => "?".into(),
    }
}

/// `1.2k` for stars.
pub fn compact_count(n: i64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}
