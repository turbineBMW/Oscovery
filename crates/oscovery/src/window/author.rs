//! "More by this developer": everything in the catalog by one author.

use super::OscoveryWindow;
use crate::i18n::{format, gettext, plural};
use crate::widgets;
use gtk::prelude::*;

pub fn page(window: &OscoveryWindow, author_key: &str, author: &str) -> adw::NavigationPage {
    let entries = window
        .db()
        .borrow()
        .by_author(author_key, "")
        .unwrap_or_default();
    let title = if author.is_empty() {
        author_key.to_string()
    } else {
        author.to_string()
    };

    let count = gtk::Label::builder()
        .label(format(
            &plural("{n} project", "{n} projects", entries.len()),
            &[("n", &entries.len().to_string())],
        ))
        .xalign(0.0)
        .margin_start(18)
        .margin_top(12)
        .css_classes(["dim-label"])
        .build();
    let flow = gtk::FlowBox::builder()
        .homogeneous(true)
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(1)
        .max_children_per_line(8)
        .column_spacing(12)
        .row_spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(18)
        .valign(gtk::Align::Start)
        .build();
    widgets::open_on_activate(&flow, window);
    widgets::fill_cards(&flow, &entries);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&count);
    content.append(&flow);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&content)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroller));
    adw::NavigationPage::builder()
        .title(format(&gettext("By {author}"), &[("author", &title)]))
        .child(&toolbar)
        .build()
}
