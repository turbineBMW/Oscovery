//! gettext setup and the sentences the core's errors turn into.

use crate::config::GETTEXT_DOMAIN;
pub use gettextrs::gettext;
use gettextrs::{LocaleCategory, bindtextdomain, setlocale, textdomain};
use oscovery_core::error::Error;

pub fn init() {
    // SAFETY: called once at startup, before any other thread exists.
    unsafe {
        setlocale(LocaleCategory::LcAll, "");
    }
    let _ = bindtextdomain(GETTEXT_DOMAIN, locale_dir());
    let _ = textdomain(GETTEXT_DOMAIN);
}

/// Where install.sh put the translations: `share/locale` beside the `bin`
/// the program runs from (~/.local by default, /usr system-wide). A
/// checkout run finds none there and shows the source strings.
fn locale_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("share/locale")))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| "/usr/share/locale".into())
}

/// `gettext` with `{name}` placeholders substituted, in a form that keeps the
/// placeholders visible to translators.
pub fn format(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut text = template.to_string();
    for (key, value) in pairs {
        text = text.replace(&format!("{{{key}}}"), value);
    }
    text
}

/// `singular` or `plural` as the language counts `n` (`ngettext`); the
/// pot script extracts both.
pub fn plural(singular: &str, plural: &str, n: usize) -> String {
    gettextrs::ngettext(singular, plural, u32::try_from(n).unwrap_or(u32::MAX))
}

/// What to tell the user about an error, in a sentence.
pub fn failure_message(error: &Error) -> String {
    match error {
        Error::Http(_) => gettext("Could not fetch the catalog. Check your connection."),
        Error::Decode(_) => gettext("The catalog index did not parse."),
        Error::IndexVersion(..) => {
            gettext("The catalog index is newer than this Oscovery understands. Update Oscovery.")
        }
        Error::Db(error) => format(
            &gettext("Database error: {error}"),
            &[("error", &error.to_string())],
        ),
        Error::Io(error) => error.to_string(),
        Error::Message(text) => text.clone(),
    }
}
