//! Preferences: the coding agent, the catalog source, appearance.

use crate::application::OscoveryApplication;
use crate::i18n::gettext;
use crate::settings as keys;
use crate::window::OscoveryWindow;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

/// The agents Preferences offers, in the order they are tried when the
/// setting is empty. Ids match the `harness` setting's choices.
const HARNESSES: [(&str, &str); 4] = [
    ("claude", "Claude Code"),
    ("codex", "Codex"),
    ("opencode", "opencode"),
    ("pi", "pi"),
];

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/turbinebmw/Oscovery/ui/preferences-dialog.ui")]
    pub struct PreferencesDialog {
        #[template_child]
        pub harness_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub model_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub index_url_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub refresh_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub follow_omarchy_row: TemplateChild<adw::SwitchRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesDialog {
        const NAME: &'static str = "OscoveryPreferencesDialog";
        type Type = super::PreferencesDialog;
        type ParentType = adw::PreferencesDialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PreferencesDialog {}
    impl WidgetImpl for PreferencesDialog {}
    impl AdwDialogImpl for PreferencesDialog {}
    impl PreferencesDialogImpl for PreferencesDialog {}
}

glib::wrapper! {
    pub struct PreferencesDialog(ObjectSubclass<imp::PreferencesDialog>)
        @extends adw::PreferencesDialog, adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl PreferencesDialog {
    pub fn new(app: &OscoveryApplication, window: &OscoveryWindow) -> Self {
        let dialog: Self = glib::Object::new();
        let imp = dialog.imp();
        let settings = app.settings();

        // Agent: "Automatic" first, then each one, with "(not installed)"
        // where the executable is missing.
        let mut labels: Vec<String> = vec![gettext("Automatic (first installed)")];
        for (id, label) in HARNESSES {
            labels.push(if installed(id) {
                label.to_string()
            } else {
                crate::i18n::format(&gettext("{agent} (not installed)"), &[("agent", label)])
            });
        }
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        imp.harness_row
            .set_model(Some(&gtk::StringList::new(&label_refs)));
        let current = settings.string(keys::HARNESS);
        imp.harness_row.set_selected(
            HARNESSES
                .iter()
                .position(|(id, _)| *id == current.as_str())
                .map_or(0, |i| i as u32 + 1),
        );
        imp.harness_row.connect_selected_notify(glib::clone!(
            #[strong]
            settings,
            move |row| {
                let id = match row.selected() {
                    0 => "",
                    n => HARNESSES[(n - 1) as usize].0,
                };
                let _ = settings.set_string(keys::HARNESS, id);
            }
        ));
        settings.bind(keys::MODEL, &*imp.model_row, "text").build();
        settings
            .bind(keys::INDEX_URL, &*imp.index_url_row, "text")
            .build();
        settings
            .bind(
                keys::FOLLOW_OMARCHY_THEME,
                &*imp.follow_omarchy_row,
                "active",
            )
            .build();
        imp.follow_omarchy_row
            .set_sensitive(crate::omarchy::detected());
        if let Some(name) = crate::omarchy::theme_name() {
            imp.follow_omarchy_row.set_subtitle(&crate::i18n::format(
                &gettext("The current theme is {theme}."),
                &[("theme", &name)],
            ));
        }
        if let Some(at) = window
            .db()
            .borrow()
            .meta("index-fetched-at")
            .ok()
            .flatten()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok())
        {
            imp.refresh_row.set_subtitle(&crate::i18n::format(
                &gettext("Last checked {when}"),
                &[(
                    "when",
                    &at.with_timezone(&chrono::Local)
                        .format("%-d %b %Y, %H:%M")
                        .to_string(),
                )],
            ));
        }
        dialog
    }
}

/// Whether the agent's executable is on PATH or in ~/.local/bin, where
/// these tools install themselves but a desktop session may not look.
fn installed(id: &str) -> bool {
    let local_bin = glib::home_dir().join(".local/bin").join(id);
    glib::find_program_in_path(id).is_some() || local_bin.is_file()
}
