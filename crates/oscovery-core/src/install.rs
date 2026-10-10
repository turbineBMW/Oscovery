//! Installing, enabling and removing: what each entry's install method
//! turns into, and the installed state read back from Omarchy and pacman.
//! Nothing here runs silently: the GTK side shows the command first. Plugins
//! go through `omarchy plugin add` (a clone, no code executed, no sudo);
//! packages run in a floating terminal so the user sees the prompt.

use crate::error::{Error, Result};
use crate::models::{Entry, InstallMethod};
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::Command;

/// How an entry gets onto this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// A child process whose output comes back when it exits.
    Run { program: String, args: Vec<String> },
    /// A floating terminal showing a shell command (needs a password, or
    /// a build worth watching).
    Terminal { command: String },
    /// Nothing automatic: the page links to the instructions.
    Manual,
}

impl Action {
    /// What the user sees before agreeing.
    pub fn display(&self) -> String {
        match self {
            Action::Run { program, args } => {
                let mut parts = vec![program.clone()];
                parts.extend(args.iter().cloned());
                parts.join(" ")
            }
            Action::Terminal { command } => command.clone(),
            Action::Manual => String::new(),
        }
    }
}

/// The install action for an entry, from its index metadata.
pub fn install_action(entry: &Entry) -> Action {
    let Some(install) = &entry.install else {
        return Action::Manual;
    };
    match install.method {
        InstallMethod::OmarchyPlugin => match plugin_repo(&install.command, &entry.repo) {
            Some(repo) => Action::Run {
                program: "omarchy-plugin-add".into(),
                args: vec![repo, "--enable".into(), "--yes".into()],
            },
            None => Action::Manual,
        },
        InstallMethod::OmarchyBuiltin => Action::Run {
            program: "omarchy-plugin-enable".into(),
            args: vec![entry.id.clone()],
        },
        InstallMethod::Aur => match package_name(&entry.id, "aur:") {
            Some(name) => Action::Terminal {
                command: format!("yay -S --needed {name}"),
            },
            None => Action::Manual,
        },
        InstallMethod::Pacman => match package_name(&entry.id, "pacman:") {
            Some(name) => Action::Terminal {
                command: format!("omarchy pkg add {name}"),
            },
            None => Action::Manual,
        },
        InstallMethod::InstallScript if !entry.repo.is_empty() => Action::Terminal {
            command: format!(
                "git clone {} && cd {} && sh install.sh",
                shell_quote(&entry.repo),
                shell_quote(repo_dir_name(&entry.repo))
            ),
        },
        InstallMethod::InstallScript | InstallMethod::GithubRelease | InstallMethod::Manual => {
            Action::Manual
        }
    }
}

pub fn enable_action(id: &str) -> Action {
    Action::Run {
        program: "omarchy-plugin-enable".into(),
        args: vec![id.to_string()],
    }
}

pub fn disable_action(id: &str) -> Action {
    Action::Run {
        program: "omarchy-plugin-disable".into(),
        args: vec![id.to_string()],
    }
}

pub fn remove_action(id: &str) -> Action {
    Action::Run {
        program: "omarchy-plugin-remove".into(),
        args: vec![id.to_string(), "--yes".into()],
    }
}

/// The git URL out of an `omarchy plugin add <url> ...` command, or the
/// entry's repository.
fn plugin_repo(command: &str, repo: &str) -> Option<String> {
    let from_command = command
        .split_whitespace()
        .skip_while(|word| *word != "add")
        .nth(1)
        .filter(|word| word.starts_with("http") || word.starts_with("git@"));
    from_command
        .map(str::to_string)
        .or_else(|| (!repo.is_empty()).then(|| repo.to_string()))
}

fn package_name<'a>(id: &'a str, prefix: &str) -> Option<&'a str> {
    id.strip_prefix(prefix).filter(|name| {
        !name.is_empty()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '+' | '@'))
    })
}

fn repo_dir_name(repo: &str) -> &str {
    let name = repo
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(repo);
    name.strip_suffix(".git").unwrap_or(name)
}

/// Single-quote for `sh`.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// What a finished child process said.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    /// The last line of complaint, for a toast.
    pub fn complaint(&self) -> String {
        last_line(&self.stderr)
            .or_else(|| last_line(&self.stdout))
            .unwrap_or_else(|| "the command failed".into())
    }
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}

/// Run a `Run` action to completion (no terminal, no prompts: `--yes` is
/// always passed where the command would ask).
pub async fn run(action: &Action) -> Result<Outcome> {
    match action {
        Action::Run { program, args } => {
            let output = Command::new(program)
                .args(args)
                .env("GIT_TERMINAL_PROMPT", "0")
                .stdin(Stdio::null())
                .output()
                .await
                .map_err(|error| Error::message(format!("could not start {program}: {error}")))?;
            Ok(Outcome {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
        Action::Terminal { command } => {
            // The terminal takes over; we only learn whether it opened.
            let status = Command::new("omarchy-launch-floating-terminal-with-presentation")
                .arg(command)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map_err(|error| Error::message(format!("could not open a terminal: {error}")))?;
            Ok(Outcome {
                success: status.success(),
                ..Default::default()
            })
        }
        Action::Manual => Err(Error::message("nothing to run")),
    }
}

/// An installed plugin as `omarchy-plugin-list --json` reports it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub first_party: bool,
    #[serde(default)]
    pub can_disable: bool,
}

/// Every plugin Omarchy knows, by id. Empty (not an error) off Omarchy.
pub async fn installed_plugins() -> HashMap<String, InstalledPlugin> {
    let output = Command::new("omarchy-plugin-list")
        .arg("--json")
        .stdin(Stdio::null())
        .output()
        .await;
    let Ok(output) = output else {
        return HashMap::new();
    };
    parse_plugin_list(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_plugin_list(json: &str) -> HashMap<String, InstalledPlugin> {
    serde_json::from_str::<Vec<InstalledPlugin>>(json)
        .unwrap_or_default()
        .into_iter()
        .map(|plugin| (plugin.id.clone(), plugin))
        .collect()
}

/// Whether a package is installed, by `pacman -Q`.
pub async fn package_installed(name: &str) -> bool {
    Command::new("pacman")
        .args(["-Q", name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Install, Kind};

    fn entry(id: &str, method: InstallMethod, command: &str, repo: &str) -> Entry {
        Entry {
            id: id.into(),
            kind: Kind::Plugin,
            subkind: String::new(),
            name: "X".into(),
            summary: String::new(),
            description: String::new(),
            author: String::new(),
            author_key: String::new(),
            repo: repo.into(),
            homepage: String::new(),
            listing_url: String::new(),
            license: String::new(),
            stars: 0,
            category: String::new(),
            tags: Vec::new(),
            version: String::new(),
            install: Some(Install {
                method,
                command: command.into(),
                note: String::new(),
            }),
            previews: Vec::new(),
            thumbnail: String::new(),
            first_seen: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            upstream_added: None,
            updated_at: None,
            wants_contributors: false,
            source: String::new(),
            status: String::new(),
            verified: false,
        }
    }

    #[test]
    fn plugins_install_through_omarchy_without_prompts() {
        let action = install_action(&entry(
            "a.b",
            InstallMethod::OmarchyPlugin,
            "omarchy plugin add https://github.com/a/b.git --enable",
            "https://github.com/a/b",
        ));
        assert_eq!(
            action,
            Action::Run {
                program: "omarchy-plugin-add".into(),
                args: vec![
                    "https://github.com/a/b.git".into(),
                    "--enable".into(),
                    "--yes".into()
                ]
            }
        );
        assert_eq!(
            action.display(),
            "omarchy-plugin-add https://github.com/a/b.git --enable --yes"
        );
        let builtin = install_action(&entry("omarchy.x", InstallMethod::OmarchyBuiltin, "", ""));
        assert_eq!(builtin.display(), "omarchy-plugin-enable omarchy.x");
    }

    #[test]
    fn packages_go_to_a_terminal_and_odd_names_do_not() {
        let aur = install_action(&entry("aur:foo-bin", InstallMethod::Aur, "", ""));
        assert_eq!(aur.display(), "yay -S --needed foo-bin");
        let bad = install_action(&entry("aur:foo; rm -rf /", InstallMethod::Aur, "", ""));
        assert_eq!(bad, Action::Manual);
        let script = install_action(&entry(
            "gh:t/Rustle",
            InstallMethod::InstallScript,
            "",
            "https://github.com/t/Rustle",
        ));
        assert_eq!(
            script.display(),
            "git clone 'https://github.com/t/Rustle' && cd 'Rustle' && sh install.sh"
        );
    }

    #[test]
    fn the_plugin_list_parses() {
        let list = parse_plugin_list(
            r#"[{"id":"a.b","name":"B","kinds":["bar-widget"],"enabled":true,"active":true,"canDisable":true,"firstParty":false,"clonedFrom":""}]"#,
        );
        assert!(list["a.b"].enabled);
        assert!(parse_plugin_list("not json").is_empty());
    }
}
