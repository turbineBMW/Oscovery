//! Discover: describe what you want; the agent searches the catalog and
//! answers; the conversation goes on from there. Matches render as cards
//! under the answer; an "extend" offer becomes a clone-and-hand-off button.

use super::OscoveryWindow;
use crate::agent;
use crate::i18n::{format, gettext};
use crate::widgets;
use crate::{rt, settings as keys};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use oscovery_core::harness::{Answer, Harness, Offer, Role, Turn};
use oscovery_core::models::Entry;
use std::cell::{Cell, RefCell};
use std::time::Instant;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/turbinebmw/Oscovery/ui/discover-page.ui")]
    pub struct DiscoverPage {
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub conversation: TemplateChild<gtk::Box>,
        #[template_child]
        pub intro: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub prompt_entry: TemplateChild<gtk::Entry>,
        #[template_child]
        pub send_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub reset_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub busy_spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        pub status_label: TemplateChild<gtk::Label>,

        pub window: glib::WeakRef<OscoveryWindow>,
        pub turns: RefCell<Vec<Turn>>,
        pub busy: Cell<bool>,
        /// The last request the person typed, for the extend hand-off.
        pub last_request: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DiscoverPage {
        const NAME: &'static str = "OscoveryDiscoverPage";
        type Type = super::DiscoverPage;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for DiscoverPage {}
    impl WidgetImpl for DiscoverPage {}
    impl BoxImpl for DiscoverPage {}
}

glib::wrapper! {
    pub struct DiscoverPage(ObjectSubclass<imp::DiscoverPage>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl DiscoverPage {
    pub fn setup(&self, window: &OscoveryWindow) {
        let imp = self.imp();
        imp.window.set(Some(window));
        imp.send_button.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.send()
        ));
        imp.prompt_entry.connect_activate(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.send()
        ));
        imp.reset_button.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.reset()
        ));
        window.settings().connect_changed(
            Some(keys::HARNESS),
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_, _| page.refresh_agent()
            ),
        );
        self.refresh_agent();
    }

    /// Which agent answers, shown under the box; no agent shows the help.
    pub fn refresh_agent(&self) {
        let imp = self.imp();
        let Some(window) = imp.window.upgrade() else {
            return;
        };
        let (harness, model) = agent::configured(window.settings());
        match harness {
            Some(harness) => {
                imp.stack.set_visible_child_name("chat");
                if !imp.busy.get() {
                    imp.status_label.set_text(&agent_line(harness, &model));
                }
            }
            None => imp.stack.set_visible_child_name("no-agent"),
        }
    }

    pub fn focus_prompt(&self) {
        self.imp().prompt_entry.grab_focus();
    }

    fn reset(&self) {
        let imp = self.imp();
        if imp.busy.get() {
            return;
        }
        imp.turns.borrow_mut().clear();
        imp.last_request.borrow_mut().clear();
        while let Some(child) = imp.conversation.last_child() {
            if child == *imp.intro {
                break;
            }
            imp.conversation.remove(&child);
        }
        imp.intro.set_visible(true);
        imp.reset_button.set_sensitive(false);
        imp.prompt_entry.grab_focus();
    }

    fn send(&self) {
        let imp = self.imp();
        if imp.busy.get() {
            return;
        }
        let question = imp.prompt_entry.text().trim().to_string();
        if question.is_empty() {
            return;
        }
        let Some(window) = imp.window.upgrade() else {
            return;
        };
        let (Some(harness), model) = agent::configured(window.settings()) else {
            self.refresh_agent();
            return;
        };
        imp.prompt_entry.set_text("");
        imp.intro.set_visible(false);
        self.append_bubble(&question, "user");
        if imp.turns.borrow().is_empty() {
            imp.last_request.replace(question.clone());
        }
        let turns = imp.turns.borrow().clone();
        self.set_busy(true, harness);
        let started = Instant::now();
        let tick = glib::timeout_add_local(
            std::time::Duration::from_secs(1),
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    page.imp().status_label.set_text(&format(
                        &gettext("{agent} is searching the catalog… {s}s"),
                        &[
                            ("agent", harness.label()),
                            ("s", &started.elapsed().as_secs().to_string()),
                        ],
                    ));
                    glib::ControlFlow::Continue
                }
            ),
        );
        agent::ask(
            window.db(),
            harness,
            model,
            turns,
            question.clone(),
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |result| {
                    tick.remove();
                    page.set_busy(false, harness);
                    match result {
                        Ok((answer, entries)) => {
                            let imp = page.imp();
                            imp.turns.borrow_mut().push(Turn {
                                role: Role::User,
                                text: question,
                            });
                            imp.turns.borrow_mut().push(Turn {
                                role: Role::Assistant,
                                text: answer.text.clone(),
                            });
                            imp.reset_button.set_sensitive(true);
                            page.append_answer(&answer, &entries, harness);
                        }
                        Err(error) => {
                            tracing::warn!("{} failed: {error}", harness.label());
                            page.append_bubble(
                                &format(
                                    &gettext("{agent} could not answer: {error}"),
                                    &[("agent", harness.label()), ("error", &error)],
                                ),
                                "error",
                            );
                        }
                    }
                }
            ),
        );
    }

    fn set_busy(&self, busy: bool, harness: Harness) {
        let imp = self.imp();
        imp.busy.set(busy);
        imp.busy_spinner.set_visible(busy);
        imp.send_button.set_sensitive(!busy);
        imp.prompt_entry.set_sensitive(!busy);
        imp.reset_button
            .set_sensitive(!busy && !imp.turns.borrow().is_empty());
        if !busy {
            let model = imp
                .window
                .upgrade()
                .map(|w| w.settings().string(keys::MODEL).to_string())
                .unwrap_or_default();
            imp.status_label.set_text(&agent_line(harness, &model));
            imp.prompt_entry.grab_focus();
        }
    }

    fn append_bubble(&self, text: &str, class: &str) -> gtk::Label {
        let imp = self.imp();
        let label = gtk::Label::builder()
            .label(text)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.0)
            .selectable(true)
            .css_classes(["bubble", class])
            .halign(if class == "user" {
                gtk::Align::End
            } else {
                gtk::Align::Fill
            })
            .build();
        if class == "user" {
            label.set_max_width_chars(60);
        }
        imp.conversation.append(&label);
        self.scroll_to_end();
        label
    }

    fn append_answer(&self, answer: &Answer, entries: &[Entry], harness: Harness) {
        let imp = self.imp();
        let Some(window) = imp.window.upgrade() else {
            return;
        };
        let label = self.append_bubble("", "assistant");
        label.set_markup(&markdown_to_markup(&answer.text));

        if !entries.is_empty() {
            let flow = gtk::FlowBox::builder()
                .homogeneous(true)
                .selection_mode(gtk::SelectionMode::None)
                .min_children_per_line(1)
                .max_children_per_line(6)
                .column_spacing(12)
                .row_spacing(12)
                .valign(gtk::Align::Start)
                .build();
            widgets::open_on_activate(&flow, &window);
            widgets::fill_cards(&flow, entries);
            imp.conversation.append(&flow);
        }

        if answer.offer == Offer::Extend
            && let Some(closest) = entries.first()
            && !closest.repo.is_empty()
        {
            let row = gtk::Box::builder().spacing(8).build();
            let button = gtk::Button::builder()
                .css_classes(["pill", "suggested-action"])
                .child(&adw::ButtonContent::builder()
                    .icon_name("document-edit-symbolic")
                    .label(format(
                        &gettext("Clone {name} and add this with {agent}"),
                        &[("name", &closest.name), ("agent", harness.label())],
                    ))
                    .build())
                .tooltip_text(gettext(
                    "Clones the project into your projects folder and opens a terminal with your agent on the task; it can open a pull request when done.",
                ))
                .build();
            let hint = gtk::Label::builder()
                .label(gettext(
                    "Nothing does exactly this yet; this project is closest.",
                ))
                .css_classes(["dim-label", "caption"])
                .wrap(true)
                .xalign(0.0)
                .valign(gtk::Align::Center)
                .build();
            let closest = closest.clone();
            button.connect_clicked(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |button| page.extend(&closest, button)
            ));
            row.append(&button);
            row.append(&hint);
            imp.conversation.append(&row);
        } else if answer.offer == Offer::Build {
            let hint = gtk::Label::builder()
                .label(gettext(
                    "Nothing in the catalog is close. If you build it, submitting it here is one issue form away.",
                ))
                .css_classes(["dim-label", "caption"])
                .wrap(true)
                .xalign(0.0)
                .build();
            imp.conversation.append(&hint);
        }
        self.scroll_to_end();
    }

    /// Clone the closest project and hand the agent the request in a terminal.
    fn extend(&self, entry: &Entry, button: &gtk::Button) {
        let imp = self.imp();
        let Some(window) = imp.window.upgrade() else {
            return;
        };
        let (Some(harness), model) = agent::configured(window.settings()) else {
            return;
        };
        let request = {
            let last = imp.last_request.borrow();
            if last.is_empty() {
                imp.turns
                    .borrow()
                    .iter()
                    .find(|t| t.role == Role::User)
                    .map(|t| t.text.clone())
                    .unwrap_or_default()
            } else {
                last.clone()
            }
        };
        let prompt = oscovery_core::harness::extend_prompt(&entry.name, &entry.repo, &request);
        let projects = agent::projects_dir(window.settings());
        let repo = entry.repo.clone();
        let name = entry.name.clone();
        button.set_sensitive(false);
        window.toast(&format(
            &gettext("Cloning {name} into {dir}…"),
            &[("name", &name), ("dir", &projects.display().to_string())],
        ));
        rt::run(
            async move {
                let dir = agent::clone_into(repo, projects).await?;
                agent::hand_off(harness, model, dir.clone(), prompt).await?;
                Ok::<_, String>(dir)
            },
            glib::clone!(
                #[weak]
                window,
                #[weak]
                button,
                move |result| {
                    button.set_sensitive(true);
                    match result {
                        Ok(dir) => window.toast(&format(
                            &gettext("{agent} is working in {dir}"),
                            &[
                                ("agent", harness.label()),
                                ("dir", &dir.display().to_string()),
                            ],
                        )),
                        Err(error) => window.toast(&error),
                    }
                }
            ),
        );
    }

    fn scroll_to_end(&self) {
        let scroller = self.imp().scroller.clone();
        glib::idle_add_local_once(move || {
            let adjustment = scroller.vadjustment();
            adjustment.set_value(adjustment.upper() - adjustment.page_size());
        });
    }
}

fn agent_line(harness: Harness, model: &str) -> String {
    if model.trim().is_empty() {
        format(
            &gettext("Answers come from {agent}, on your account."),
            &[("agent", harness.label())],
        )
    } else {
        format(
            &gettext("Answers come from {agent} ({model}), on your account."),
            &[("agent", harness.label()), ("model", model.trim())],
        )
    }
}

/// The little markdown an answer uses, as Pango markup: bold, code, and
/// bullets left as they are.
pub fn markdown_to_markup(text: &str) -> String {
    let escaped = glib::markup_escape_text(text).to_string();
    let mut out = String::with_capacity(escaped.len());
    let mut rest = escaped.as_str();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("**")
            && let Some(end) = after.find("**")
        {
            out.push_str("<b>");
            out.push_str(&after[..end]);
            out.push_str("</b>");
            rest = &after[end + 2..];
        } else if let Some(after) = rest.strip_prefix('`')
            && let Some(end) = after.find('`')
        {
            out.push_str("<tt>");
            out.push_str(&after[..end]);
            out.push_str("</tt>");
            rest = &after[end + 1..];
        } else {
            let mut chars = rest.chars();
            out.push(chars.next().unwrap());
            rest = chars.as_str();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::markdown_to_markup;

    #[test]
    fn bold_code_and_escapes() {
        assert_eq!(
            markdown_to_markup("**Batt** shows `<charge>` & more"),
            "<b>Batt</b> shows <tt>&lt;charge&gt;</tt> &amp; more"
        );
        assert_eq!(markdown_to_markup("a ** b"), "a ** b");
    }
}
