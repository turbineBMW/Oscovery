//! "Review, then install": a page that clones the project, has the agent
//! read it, and shows the verdict with the Install button underneath.

use super::OscoveryWindow;
use crate::i18n::{format, gettext};
use crate::window::discover::markdown_to_markup;
use crate::{agent, rt, widgets};
use adw::prelude::*;
use gtk::glib;
use oscovery_core::harness::Harness;
use oscovery_core::install::{self, Action};
use oscovery_core::models::Entry;
use oscovery_core::review::{self, Review, Verdict};
use std::time::Instant;

pub fn page(window: &OscoveryWindow, entry: &Entry) -> adw::NavigationPage {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(18)
        .margin_end(18)
        .margin_top(12)
        .margin_bottom(24)
        .build();

    let (harness, model) = agent::configured(window.settings());
    let source = review::source_url(entry);

    let status = gtk::Box::builder().spacing(10).build();
    let spinner = adw::Spinner::new();
    let status_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .hexpand(true)
        .build();
    status.append(&spinner);
    status.append(&status_label);
    content.append(&status);

    let verdict_row = gtk::Box::builder().spacing(8).visible(false).build();
    content.append(&verdict_row);

    let findings_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .visible(false)
        .build();
    content.append(&findings_box);

    let text_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .visible(false)
        .css_classes(["bubble", "assistant"])
        .build();
    content.append(&text_label);

    let action = install::install_action(entry);
    let install_button = gtk::Button::builder()
        .css_classes(["pill", "suggested-action"])
        .halign(gtk::Align::Start)
        .sensitive(false)
        .child(
            &adw::ButtonContent::builder()
                .icon_name("folder-download-symbolic")
                .label(gettext("Install"))
                .build(),
        )
        .build();
    let command_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .css_classes(["install-command"])
        .label(action.display())
        .visible(!matches!(action, Action::Manual))
        .build();
    content.append(&install_button);
    content.append(&command_label);
    let note = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label", "caption"])
        .label(gettext(
            "The review is one reading of the code by your agent, not a guarantee. The clone it read stays in ~/.cache/oscovery/review until the next review.",
        ))
        .build();
    content.append(&note);

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(
            &adw::Clamp::builder()
                .maximum_size(1000)
                .tightening_threshold(700)
                .child(&content)
                .build(),
        )
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroller));
    let page = adw::NavigationPage::builder()
        .title(format(
            &gettext("Review of {name}"),
            &[("name", &entry.name)],
        ))
        .child(&toolbar)
        .build();

    match (harness, source, &action) {
        (None, _, _) => {
            spinner.set_visible(false);
            status_label.set_text(&gettext(
                "No coding agent found. Install Claude Code, Codex, opencode or pi, or pick one in Preferences.",
            ));
        }
        (_, None, _) => {
            spinner.set_visible(false);
            status_label.set_text(&gettext("This project has no source to review."));
        }
        (Some(harness), Some(url), action) => {
            status_label.set_text(&format(&gettext("Cloning {url}…"), &[("url", &url)]));
            let dir =
                review::clone_dir(&glib::user_cache_dir().join(crate::config::DIR_NAME), entry);
            let prompt = review::prompt(entry);
            let started = Instant::now();
            let tick = glib::timeout_add_local(
                std::time::Duration::from_secs(1),
                glib::clone!(
                    #[weak]
                    status_label,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        if started.elapsed().as_secs() > 3 {
                            status_label.set_text(&format(
                                &gettext("{agent} is reading the code… {s}s"),
                                &[
                                    ("agent", harness.label()),
                                    ("s", &started.elapsed().as_secs().to_string()),
                                ],
                            ));
                        }
                        glib::ControlFlow::Continue
                    }
                ),
            );
            let action = action.clone();
            let entry = entry.clone();
            let model = model.clone();
            rt::run(
                async move {
                    review::clone(&url, &dir)
                        .await
                        .map_err(|error| error.to_string())?;
                    tokio::task::spawn_blocking(move || {
                        oscovery_core::harness::ask(harness, &model, &prompt, &dir)
                    })
                    .await
                    .map_err(|error| error.to_string())?
                },
                glib::clone!(
                    #[weak]
                    window,
                    #[weak]
                    spinner,
                    #[weak]
                    status_label,
                    #[weak]
                    verdict_row,
                    #[weak]
                    findings_box,
                    #[weak]
                    text_label,
                    #[weak]
                    install_button,
                    move |result| {
                        tick.remove();
                        spinner.set_visible(false);
                        match result {
                            Ok(raw) => {
                                let review = review::parse(&raw);
                                show_review(
                                    &review,
                                    harness,
                                    &status_label,
                                    &verdict_row,
                                    &findings_box,
                                    &text_label,
                                );
                                install_button.set_sensitive(!matches!(action, Action::Manual));
                                let action = action.clone();
                                let entry = entry.clone();
                                install_button.connect_clicked(glib::clone!(
                                    #[weak]
                                    window,
                                    move |button| run_install(&window, &entry, &action, button)
                                ));
                            }
                            Err(error) => {
                                status_label.set_text(&format(
                                    &gettext("The review did not finish: {error}"),
                                    &[("error", &error)],
                                ));
                            }
                        }
                    }
                ),
            );
        }
    }
    page
}

fn show_review(
    review: &Review,
    harness: Harness,
    status_label: &gtk::Label,
    verdict_row: &gtk::Box,
    findings_box: &gtk::Box,
    text_label: &gtk::Label,
) {
    status_label.set_text(&format(
        &gettext("Reviewed by {agent}."),
        &[("agent", harness.label())],
    ));
    let (text, class) = match review.verdict {
        Verdict::Safe => (gettext("Nothing unexpected"), "verified"),
        Verdict::Caution => (gettext("Worth knowing before you install"), "kind"),
        Verdict::Avoid => (gettext("Avoid"), "error"),
        Verdict::Unknown => (gettext("No verdict"), "muted"),
    };
    let chip = widgets::chip(&text, class);
    chip.add_css_class("title-4");
    verdict_row.append(&chip);
    verdict_row.set_visible(true);
    for finding in &review.findings {
        let row = gtk::Box::builder().spacing(8).build();
        row.append(&widgets::chip(
            &finding.severity,
            match finding.severity.as_str() {
                "high" => "error",
                "medium" => "kind",
                _ => "muted",
            },
        ));
        let label = gtk::Label::builder()
            .label(if finding.file.is_empty() {
                finding.title.clone()
            } else {
                format!("{} ({})", finding.title, finding.file)
            })
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .build();
        row.append(&label);
        findings_box.append(&row);
    }
    findings_box.set_visible(!review.findings.is_empty());
    text_label.set_markup(&markdown_to_markup(&review.text));
    text_label.set_visible(!review.text.is_empty());
}

fn run_install(window: &OscoveryWindow, entry: &Entry, action: &Action, button: &gtk::Button) {
    let dialog = adw::AlertDialog::builder()
        .heading(format(
            &gettext("Install {name}?"),
            &[("name", &entry.name)],
        ))
        .body(action.display())
        .build();
    dialog.add_responses(&[("cancel", &gettext("Cancel")), ("go", &gettext("Install"))]);
    dialog.set_response_appearance("go", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("go"));
    let action = action.clone();
    dialog.connect_response(
        None,
        glib::clone!(
            #[weak]
            window,
            #[weak]
            button,
            move |_, response| {
                if response != "go" {
                    return;
                }
                button.set_sensitive(false);
                let action = action.clone();
                rt::run(
                    async move { install::run(&action).await },
                    glib::clone!(
                        #[weak]
                        window,
                        #[weak]
                        button,
                        move |result| {
                            button.set_sensitive(true);
                            match result {
                                Ok(outcome) if outcome.success => window.toast(&gettext("Done.")),
                                Ok(outcome) => window.toast(&outcome.complaint()),
                                Err(error) => window.toast(&error.to_string()),
                            }
                        }
                    ),
                );
            }
        ),
    );
    dialog.present(Some(window));
}
