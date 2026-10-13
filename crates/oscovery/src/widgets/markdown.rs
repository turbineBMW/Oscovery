//! A README's blocks (`oscovery_core::markdown`) as widgets, laid out the
//! way github.com lays a README out: labels with Pango markup, framed code
//! with a copy button, a grid for tables, pictures sized like the page's.

use crate::i18n::gettext;
use crate::images;
use adw::prelude::*;
use gtk::glib;
use oscovery_core::markdown::{self, AlertKind, Align, Block, Image, Item, Scheme};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// What following a link in the README does.
pub type OnLink = Rc<dyn Fn(&str)>;

pub struct Rendered {
    pub widget: gtk::Box,
    /// Headings by anchor, for the README's own `#links`.
    pub anchors: HashMap<String, gtk::Widget>,
}

struct Ctx {
    on_link: OnLink,
    anchors: RefCell<HashMap<String, gtk::Widget>>,
}

const BULLETS: [&str; 3] = ["•", "◦", "▪"];

pub fn render(blocks: &[Block], on_link: OnLink) -> Rendered {
    let ctx = Ctx {
        on_link,
        anchors: RefCell::default(),
    };
    let widget = column(&ctx, blocks, 0);
    Rendered {
        widget,
        anchors: ctx.anchors.into_inner(),
    }
}

/// A README that isn't Markdown (`.rst`, `.txt`), as its text.
pub fn plain_text(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .build()
}

fn column(ctx: &Ctx, blocks: &[Block], depth: usize) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 16);
    for (i, block) in blocks.iter().enumerate() {
        let widget = block_widget(ctx, block, depth);
        if i > 0 && matches!(block, Block::Heading { .. }) {
            widget.set_margin_top(8);
        }
        column.append(&widget);
    }
    column
}

fn block_widget(ctx: &Ctx, block: &Block, depth: usize) -> gtk::Widget {
    match block {
        Block::Heading {
            level,
            markup,
            anchor,
            align,
        } => {
            let label = text(ctx, markup, *align);
            label.add_css_class(&format!("readme-h{}", (*level).clamp(1, 6)));
            if !anchor.is_empty() {
                ctx.anchors
                    .borrow_mut()
                    .entry(anchor.clone())
                    .or_insert_with(|| label.clone().upcast());
            }
            label.upcast()
        }
        Block::Paragraph { markup, align } => text(ctx, markup, *align).upcast(),
        Block::Images { images, align } => image_row(ctx, images, *align),
        Block::Code { text, .. } => code(text),
        Block::Quote(blocks) => {
            let quote = column(ctx, blocks, depth);
            quote.add_css_class("readme-quote");
            quote.upcast()
        }
        Block::Alert { kind, blocks } => alert(ctx, *kind, blocks, depth),
        Block::List { start, items } => list(ctx, *start, items, depth),
        Block::Table {
            aligns,
            header,
            rows,
        } => table(ctx, aligns, header, rows),
        Block::Details {
            summary,
            open,
            blocks,
        } => details(ctx, summary, *open, blocks, depth),
        Block::Rule => {
            let rule = gtk::Separator::new(gtk::Orientation::Horizontal);
            rule.add_css_class("readme-rule");
            rule.upcast()
        }
    }
}

fn xalign(align: Align) -> f32 {
    match align {
        Align::Start => 0.0,
        Align::Center => 0.5,
        Align::End => 1.0,
    }
}

fn halign(align: Align) -> gtk::Align {
    match align {
        Align::Start => gtk::Align::Start,
        Align::Center => gtk::Align::Center,
        Align::End => gtk::Align::End,
    }
}

fn text(ctx: &Ctx, markup: &str, align: Align) -> gtk::Label {
    let label = gtk::Label::builder()
        .xalign(xalign(align))
        .justify(match align {
            Align::Start => gtk::Justification::Left,
            Align::Center => gtk::Justification::Center,
            Align::End => gtk::Justification::Right,
        })
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .build();
    // The core keeps markup balanced; anything Pango still refuses is shown
    // as its text rather than not at all.
    if is_valid_markup(markup) {
        label.set_markup(markup);
    } else {
        label.set_text(&markdown::plain(markup));
    }
    let on_link = ctx.on_link.clone();
    label.connect_activate_link(move |_, uri| {
        on_link(uri);
        glib::Propagation::Stop
    });
    label
}

/// Whether a label will take `markup`. Links are the label's own, not
/// Pango's, so they are checked as spans.
fn is_valid_markup(markup: &str) -> bool {
    let mut spans = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(at) = rest.find("<a ") {
        spans.push_str(&rest[..at]);
        let Some(end) = rest[at..].find('>') else {
            return false;
        };
        spans.push_str("<span>");
        rest = &rest[at + end + 1..];
    }
    spans.push_str(rest);
    let spans = spans.replace("</a>", "</span>");
    gtk::pango::parse_markup(&spans, '\0').is_ok()
}

fn image_row(ctx: &Ctx, images: &[Image], align: Align) -> gtk::Widget {
    if let [image] = images {
        let picture = picture(ctx, image);
        picture.set_halign(halign(align));
        return picture;
    }
    let row = adw::WrapBox::builder()
        .child_spacing(4)
        .line_spacing(4)
        .align(xalign(align))
        .build();
    for image in images {
        row.append(&picture(ctx, image));
    }
    row.upcast()
}

/// An image at its own size, or the size its `width`/`height` asks for,
/// never wider than the README; hidden until it loads, and while the
/// colour scheme is one it isn't for.
fn picture(ctx: &Ctx, image: &Image) -> gtk::Widget {
    let picture = gtk::Picture::builder()
        .can_shrink(true)
        .content_fit(gtk::ContentFit::ScaleDown)
        .alternative_text(image.alt.as_str())
        .build();
    let width = image.width.map_or(100_000, |width| width as i32);
    let clamp = adw::Clamp::builder()
        .maximum_size(width)
        .tightening_threshold(width)
        .unit(adw::LengthUnit::Px)
        .child(&picture)
        .visible(false)
        .build();
    if let Some(link) = image.link.clone() {
        let click = gtk::GestureClick::new();
        let on_link = ctx.on_link.clone();
        let target = link.clone();
        click.connect_released(move |gesture, _, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            on_link(&target);
        });
        clamp.add_controller(click);
        clamp.set_cursor_from_name(Some("pointer"));
        clamp.set_tooltip_text(Some(&link));
    }

    let style = adw::StyleManager::default();
    let loaded = Rc::new(Cell::new(false));
    let scheme = image.scheme;
    let update = Rc::new(glib::clone!(
        #[weak]
        clamp,
        #[weak]
        style,
        #[strong]
        loaded,
        move || {
            let wanted = match scheme {
                Scheme::Any => true,
                Scheme::Dark => style.is_dark(),
                Scheme::Light => !style.is_dark(),
            };
            clamp.set_visible(loaded.get() && wanted);
        }
    ));
    let height = image.height;
    let load = glib::clone!(
        #[weak]
        picture,
        #[weak]
        clamp,
        #[strong]
        update,
        move |url: &str| {
            let loaded = loaded.clone();
            let update = update.clone();
            images::load_into(
                &picture,
                url,
                glib::clone!(
                    #[weak]
                    picture,
                    #[weak]
                    clamp,
                    move || {
                        // A height alone: the width that keeps the aspect.
                        if let (None, Some(height), Some(paintable)) =
                            (clamp_width(&clamp), height, picture.paintable())
                            && paintable.intrinsic_height() > 0
                        {
                            let width = paintable.intrinsic_width() as i64 * height as i64
                                / paintable.intrinsic_height() as i64;
                            clamp.set_maximum_size(width as i32);
                            clamp.set_tightening_threshold(width as i32);
                        }
                        loaded.set(true);
                        update();
                    }
                ),
            );
        }
    );
    load(image.src_for(style.is_dark()));

    if image.scheme != Scheme::Any || image.dark_src.is_some() {
        let image = image.clone();
        let handler = style.connect_dark_notify(move |style| {
            if image.dark_src.is_some() {
                load(image.src_for(style.is_dark()));
            }
            update();
        });
        let handler = Cell::new(Some(handler));
        clamp.connect_destroy(glib::clone!(
            #[weak]
            style,
            move |_| {
                if let Some(handler) = handler.take() {
                    style.disconnect(handler);
                }
            }
        ));
    }
    clamp.upcast()
}

/// The width an image's `width` asked for, if it did.
fn clamp_width(clamp: &adw::Clamp) -> Option<i32> {
    (clamp.maximum_size() < 100_000).then(|| clamp.maximum_size())
}

fn code(text: &str) -> gtk::Widget {
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .selectable(true)
        .build();
    label.add_css_class("readme-code-text");
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&label)
        .build();
    let copy = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text(gettext("Copy"))
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_top(6)
        .margin_end(6)
        .build();
    copy.add_css_class("flat");
    let text = text.to_string();
    copy.connect_clicked(move |button| {
        button.clipboard().set_text(&text);
        button.set_icon_name("object-select-symbolic");
        glib::timeout_add_local_once(
            Duration::from_millis(1500),
            glib::clone!(
                #[weak]
                button,
                move || button.set_icon_name("edit-copy-symbolic")
            ),
        );
    });
    let overlay = gtk::Overlay::builder().child(&scroller).build();
    overlay.add_overlay(&copy);
    overlay.add_css_class("readme-code");
    overlay.upcast()
}

fn alert(ctx: &Ctx, kind: AlertKind, blocks: &[Block], depth: usize) -> gtk::Widget {
    let (class, icon, title) = match kind {
        AlertKind::Note => ("note", "dialog-information-symbolic", gettext("Note")),
        AlertKind::Tip => ("tip", "emoji-objects-symbolic", gettext("Tip")),
        AlertKind::Important => (
            "important",
            "emblem-important-symbolic",
            gettext("Important"),
        ),
        AlertKind::Warning => ("warning", "dialog-warning-symbolic", gettext("Warning")),
        AlertKind::Caution => ("caution", "dialog-error-symbolic", gettext("Caution")),
    };
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    head.add_css_class("readme-alert-title");
    head.append(&gtk::Image::from_icon_name(icon));
    head.append(&gtk::Label::new(Some(&title)));
    let alert = gtk::Box::new(gtk::Orientation::Vertical, 8);
    alert.add_css_class("readme-alert");
    alert.add_css_class(class);
    alert.append(&head);
    alert.append(&column(ctx, blocks, depth));
    alert.upcast()
}

fn list(ctx: &Ctx, start: Option<u64>, items: &[Item], depth: usize) -> gtk::Widget {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let last = start.unwrap_or(1) + items.len() as u64;
    for (i, item) in items.iter().enumerate() {
        let marker: gtk::Widget = match (item.checked, start) {
            (Some(checked), _) => gtk::CheckButton::builder()
                .active(checked)
                .sensitive(false)
                .build()
                .upcast(),
            (None, Some(start)) => gtk::Label::builder()
                .label(format!("{}.", start + i as u64))
                .width_chars(last.to_string().len() as i32 + 1)
                .xalign(1.0)
                .build()
                .upcast(),
            (None, None) => gtk::Label::builder()
                .label(BULLETS[depth % BULLETS.len()])
                .width_chars(2)
                .xalign(1.0)
                .build()
                .upcast(),
        };
        marker.set_valign(gtk::Align::Start);
        let content = column(ctx, &item.blocks, depth + 1);
        content.set_spacing(8);
        content.set_hexpand(true);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&marker);
        row.append(&content);
        list.append(&row);
    }
    list.upcast()
}

fn table(ctx: &Ctx, aligns: &[Align], header: &[String], rows: &[Vec<String>]) -> gtk::Widget {
    let grid = gtk::Grid::builder().halign(gtk::Align::Start).build();
    grid.add_css_class("readme-table");
    let columns = rows
        .iter()
        .map(Vec::len)
        .chain([header.len(), aligns.len()])
        .max()
        .unwrap_or(0);
    let header = (!header.is_empty()).then_some((header, Some("head")));
    let body = rows
        .iter()
        .enumerate()
        .map(|(i, row)| (row.as_slice(), (i % 2 == 1).then_some("alt")));
    for (y, (row, class)) in header.into_iter().chain(body).enumerate() {
        for x in 0..columns {
            let align = aligns.get(x).copied().unwrap_or_default();
            let label = text(ctx, row.get(x).map_or("", String::as_str), align);
            label.set_max_width_chars(40);
            label.add_css_class("readme-cell");
            if let Some(class) = class {
                label.add_css_class(class);
            }
            grid.attach(&label, x as i32, y as i32, 1, 1);
        }
    }
    // No scroller around it: one that scrolls sideways sizes itself for
    // its child's narrowest width, the tallest the wrapped cells get. The
    // cells wrap, so the grid fits the README's width anyway.
    grid.upcast()
}

fn details(ctx: &Ctx, summary: &str, open: bool, blocks: &[Block], depth: usize) -> gtk::Widget {
    let summary = if summary.is_empty() {
        markdown::escape(&gettext("Details"))
    } else {
        summary.to_string()
    };
    let label = text(ctx, &summary, Align::Start);
    // A selectable label would take the clicks that open it.
    label.set_selectable(false);
    let content = column(ctx, blocks, depth);
    content.set_margin_top(12);
    gtk::Expander::builder()
        .label_widget(&label)
        .expanded(open)
        .child(&content)
        .build()
        .upcast()
}
