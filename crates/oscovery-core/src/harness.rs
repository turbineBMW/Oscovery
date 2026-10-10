//! The user's coding agent, run headless over the catalog directory with
//! read-only tools, and started interactively in a terminal for the work
//! that needs a person in the loop (clone and extend).
//!
//! Ported from Rustle's `assistant.rs`, with tools on: the agent greps and
//! reads the catalog itself. It runs in the catalog directory with nothing
//! else there, is killed after `ASK_TIMEOUT`, and keeps no session.

use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long one answer may take: a cold start plus a few tool calls over
/// a five-thousand-line index.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
    Opencode,
    Pi,
}

/// What a run of the tool takes: its arguments, what goes on its stdin, and
/// the file it writes its answer to when stdout carries more than that.
#[derive(Debug, PartialEq, Eq)]
pub struct Invocation {
    pub args: Vec<String>,
    pub stdin: Option<String>,
    pub answer_file: Option<PathBuf>,
}

impl Harness {
    pub const ALL: [Harness; 4] = [
        Harness::Claude,
        Harness::Codex,
        Harness::Opencode,
        Harness::Pi,
    ];

    /// The value stored in the `harness` setting.
    pub fn id(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Opencode => "opencode",
            Harness::Pi => "pi",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|harness| harness.id() == id)
    }

    /// The product's own name; not translated.
    pub fn label(self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Codex => "Codex",
            Harness::Opencode => "opencode",
            Harness::Pi => "pi",
        }
    }

    /// The executable, looked up on PATH and then in ~/.local/bin, where
    /// these tools install themselves but which a desktop session's PATH
    /// may not include.
    pub fn program(self) -> Option<PathBuf> {
        let name = self.id();
        let on_path = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .unwrap_or_default();
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        on_path
            .into_iter()
            .chain(std::iter::once(home.join(".local/bin")))
            .map(|dir| dir.join(name))
            .find(|candidate| is_executable(candidate))
    }

    pub fn installed(self) -> bool {
        self.program().is_some()
    }

    /// The headless form: tools on but read-only, no session, nothing of
    /// the user's own configuration. `model` is passed on when set.
    pub fn search_invocation(self, model: &str, prompt: &str, scratch: &Path) -> Invocation {
        let mut args: Vec<String> = Vec::new();
        let mut stdin = None;
        let mut answer_file = None;
        let model_flag = match self {
            Harness::Claude => {
                // Only the file tools, all pre-approved; anything else is
                // refused in print mode. No session file, none of the
                // user's settings, hooks or MCP servers.
                args.extend(
                    [
                        "-p",
                        "--output-format",
                        "text",
                        "--tools",
                        "Read,Grep,Glob",
                        "--allowedTools",
                        "Read,Grep,Glob",
                        "--no-session-persistence",
                        "--setting-sources",
                        "",
                        "--strict-mcp-config",
                    ]
                    .map(String::from),
                );
                stdin = Some(prompt.to_string());
                "--model"
            }
            Harness::Codex => {
                // Its stdout is a transcript; the answer alone goes to -o.
                let file = scratch.join("answer.txt");
                args.extend(
                    [
                        "exec",
                        "--sandbox",
                        "read-only",
                        "--skip-git-repo-check",
                        "--ephemeral",
                        "--color",
                        "never",
                        "-o",
                    ]
                    .map(String::from),
                );
                args.push(file.to_string_lossy().into_owned());
                answer_file = Some(file);
                stdin = Some(prompt.to_string());
                "-m"
            }
            // opencode and pi have no read-only switch; they run in the
            // catalog directory, where there is nothing to break.
            Harness::Opencode => {
                args.push("run".into());
                "-m"
            }
            Harness::Pi => {
                args.extend(["-p", "--no-session"].map(String::from));
                "--model"
            }
        };
        if !model.trim().is_empty() {
            args.push(model_flag.into());
            args.push(model.trim().into());
        }
        match self {
            Harness::Codex => args.push("-".into()),
            Harness::Opencode | Harness::Pi => args.push(prompt.to_string()),
            Harness::Claude => {}
        }
        Invocation {
            args,
            stdin,
            answer_file,
        }
    }

    /// The interactive form, for a terminal: the agent with all its tools
    /// and the user watching, started on `prompt`.
    pub fn interactive_args(self, model: &str, prompt: &str) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        let model_flag = match self {
            Harness::Claude => "--model",
            Harness::Codex => "-m",
            Harness::Opencode => "-m",
            Harness::Pi => "--model",
        };
        if !model.trim().is_empty() {
            args.push(model_flag.into());
            args.push(model.trim().into());
        }
        match self {
            Harness::Opencode => {
                args.push("--prompt".into());
                args.push(prompt.to_string());
            }
            Harness::Claude | Harness::Codex | Harness::Pi => args.push(prompt.to_string()),
        }
        args
    }
}

/// The agent the setting names, if installed, else the first installed one.
pub fn pick(setting: &str) -> Option<Harness> {
    Harness::parse(setting)
        .filter(|harness| harness.installed())
        .or_else(|| Harness::ALL.into_iter().find(|harness| harness.installed()))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Run the tool once in `workdir` and return its answer. Blocking: call it
/// from a worker. The error is a sentence for the user (the tool's last
/// line of complaint, usually "not logged in" or similar).
pub fn ask(harness: Harness, model: &str, prompt: &str, workdir: &Path) -> Result<String, String> {
    let program = harness
        .program()
        .ok_or_else(|| format!("{} is not installed", harness.label()))?;
    let scratch = std::env::temp_dir().join(format!("oscovery-ask-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).map_err(|error| error.to_string())?;
    let result = run(
        &program,
        harness.search_invocation(model, prompt, &scratch),
        workdir,
    );
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

fn run(program: &Path, invocation: Invocation, workdir: &Path) -> Result<String, String> {
    tracing::debug!(
        "{} {:?} in {}",
        program.display(),
        invocation.args,
        workdir.display()
    );
    let mut child = Command::new(program)
        .args(&invocation.args)
        .current_dir(workdir)
        .stdin(if invocation.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", program.display()))?;
    if let (Some(text), Some(mut pipe)) = (invocation.stdin, child.stdin.take()) {
        // A thread, so a tool that reads slowly can't block us on a full pipe.
        std::thread::spawn(move || {
            let _ = pipe.write_all(text.as_bytes());
        });
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let deadline = Instant::now() + ASK_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("no answer within five minutes".into());
            }
            Err(error) => return Err(error.to_string()),
        }
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        let complaint = last_line(&strip_ansi(&stderr))
            .or_else(|| last_line(&strip_ansi(&stdout)))
            .unwrap_or_else(|| status.to_string());
        return Err(complaint);
    }
    let answer = match invocation.answer_file {
        Some(file) => std::fs::read_to_string(file).map_err(|error| error.to_string())?,
        None => stdout,
    };
    Ok(strip_ansi(&answer).trim().to_string())
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            text = String::from_utf8_lossy(&bytes).into_owned();
        }
        text
    })
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}

/// Drop ANSI escape sequences (colours, cursor moves) a tool may print.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    let mut last = '\0';
                    for next in chars.by_ref() {
                        if next == '\x07' || (last == '\x1b' && next == '\\') {
                            break;
                        }
                        last = next;
                    }
                }
                _ => {}
            }
            continue;
        }
        out.push(ch);
    }
    out
}

// --- the conversation ---------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}

/// What the agent is told, once, at the top of every prompt.
pub const INSTRUCTIONS: &str = "\
You are Oscovery's catalog assistant. The person is looking for something for their \
Omarchy desktop (an Arch Linux + Hyprland distribution with its own shell: a bar, panels, \
overlays, menus and background services, all extensible with plugins). Find what they \
describe in the catalog of Omarchy plugins and apps in the current directory.

The catalog: `INDEX.md` lists every project on one line \
(`id — name [kind, category]: summary`); `catalog/<kind>/<id>.md` has each project in \
full. Grep INDEX.md for words and synonyms, then read the pages of the candidates. A \
shortlist from a plain keyword search is included below as a starting point; it can \
miss the right projects and include wrong ones, so look further when it is thin.

Answer for a person, briefly, in plain text (no headings): which projects fit and why, \
one or two lines each, best first; say plainly when nothing fits; ask one question if \
the request is ambiguous. Name projects by their name, not their id.

Then end with exactly one fenced block tagged `oscovery` holding JSON:
```oscovery
{\"matches\": [{\"id\": \"<id>\", \"why\": \"<one line>\"}], \"offer\": \"none\"}
```
`matches` lists the ids you named, best first (empty when none). `offer` is \"extend\" \
when nothing fully fits but the first match is close enough that adding the missing \
feature to it would be the right move, \"build\" when nothing is close at all, else \"none\".

Only read the catalog. Do not install, write or run anything.";

/// The whole prompt for one answer: instructions, the shortlist, the turns
/// so far, and the new question. Re-sent whole each time, which works the
/// same with every tool.
pub fn search_prompt(shortlist: &str, turns: &[Turn], question: &str) -> String {
    let mut prompt = String::from(INSTRUCTIONS);
    prompt.push_str("\n\n## Keyword shortlist\n\n");
    if shortlist.trim().is_empty() {
        prompt.push_str("(nothing matched the keywords; search INDEX.md yourself)\n");
    } else {
        prompt.push_str(shortlist);
    }
    if !turns.is_empty() {
        prompt.push_str("\n## The conversation so far\n\n");
        for turn in turns {
            let who = match turn.role {
                Role::User => "Person",
                Role::Assistant => "You",
            };
            prompt.push_str(&format!("{who}: {}\n\n", turn.text.trim()));
        }
    }
    prompt.push_str("\n## The person now says\n\n");
    prompt.push_str(question.trim());
    prompt.push('\n');
    prompt
}

/// The prompt that starts the agent interactively in a clone: add what the
/// person asked for to the closest existing project and offer it upstream.
pub fn extend_prompt(project_name: &str, repo: &str, request: &str) -> String {
    format!(
        "This checkout is {project_name} ({repo}), an existing Omarchy project. The person \
         was looking for the following and this project is the closest thing that exists, \
         so the plan is to add it here rather than start a new project:\n\n{request}\n\n\
         Read the project first and work in its style. Keep the change focused. When it \
         works, commit it and offer to open a pull request upstream (`gh pr create`, forking \
         first if needed). Ask before anything irreversible."
    )
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// The prose, without the JSON block.
    pub text: String,
    pub matches: Vec<Match>,
    pub offer: Offer,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Match {
    pub id: String,
    #[serde(default)]
    pub why: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Offer {
    #[default]
    None,
    Extend,
    Build,
}

#[derive(Deserialize)]
struct AnswerBlock {
    #[serde(default)]
    matches: Vec<Match>,
    #[serde(default)]
    offer: String,
}

/// Split the agent's answer into prose and its structured tail. An answer
/// without the block (a question back, a refusal) is all prose.
pub fn parse_answer(raw: &str) -> Answer {
    let mut answer = Answer {
        text: raw.trim().to_string(),
        ..Default::default()
    };
    let Some((before, rest)) = raw.rsplit_once("```oscovery") else {
        return parse_bare_json(answer);
    };
    let (json, after) = rest.split_once("```").unwrap_or((rest, ""));
    if let Ok(block) = serde_json::from_str::<AnswerBlock>(json.trim()) {
        answer.matches = block.matches;
        answer.offer = match block.offer.as_str() {
            "extend" => Offer::Extend,
            "build" => Offer::Build,
            _ => Offer::None,
        };
        answer.text = format!("{}\n{}", before.trim_end(), after.trim())
            .trim()
            .to_string();
    }
    answer
}

/// Some tools wrap the block as ```json; accept one whose JSON has `matches`.
fn parse_bare_json(mut answer: Answer) -> Answer {
    let Some((before, rest)) = answer.text.rsplit_once("```json") else {
        return answer;
    };
    let (json, after) = rest.split_once("```").unwrap_or((rest, ""));
    if json.contains("\"matches\"")
        && let Ok(block) = serde_json::from_str::<AnswerBlock>(json.trim())
    {
        answer.matches = block.matches;
        answer.offer = match block.offer.as_str() {
            "extend" => Offer::Extend,
            "build" => Offer::Build,
            _ => Offer::None,
        };
        answer.text = format!("{}\n{}", before.trim_end(), after.trim())
            .trim()
            .to_string();
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_runs_with_read_only_tools_and_no_session() {
        let invocation = Harness::Claude.search_invocation("", "find x", Path::new("/tmp/s"));
        assert!(
            invocation
                .args
                .contains(&"--no-session-persistence".to_string())
        );
        assert_eq!(
            invocation
                .args
                .iter()
                .position(|a| a == "--tools")
                .map(|i| &invocation.args[i + 1]),
            Some(&"Read,Grep,Glob".to_string())
        );
        assert!(!invocation.args.iter().any(|a| a == "--model"));
        assert_eq!(invocation.stdin.as_deref(), Some("find x"));
        let with_model = Harness::Claude.search_invocation("haiku", "q", Path::new("/tmp/s"));
        assert!(
            with_model
                .args
                .windows(2)
                .any(|w| w == ["--model", "haiku"])
        );
    }

    #[test]
    fn codex_answers_through_a_file_in_a_read_only_sandbox() {
        let invocation = Harness::Codex.search_invocation("", "q", Path::new("/tmp/s"));
        assert!(
            invocation
                .args
                .windows(2)
                .any(|w| w == ["--sandbox", "read-only"])
        );
        assert_eq!(
            invocation.answer_file,
            Some(PathBuf::from("/tmp/s/answer.txt"))
        );
        assert_eq!(invocation.args.last().map(String::as_str), Some("-"));
    }

    #[test]
    fn the_prose_and_the_block_come_apart() {
        let raw = "Two fit.\n\n- Batt: shows charge.\n\n```oscovery\n{\"matches\":[{\"id\":\"a.batt\",\"why\":\"charge\"}],\"offer\":\"extend\"}\n```\n";
        let answer = parse_answer(raw);
        assert_eq!(answer.text, "Two fit.\n\n- Batt: shows charge.");
        assert_eq!(answer.matches[0].id, "a.batt");
        assert_eq!(answer.offer, Offer::Extend);

        let json_fence = "None.\n```json\n{\"matches\": [], \"offer\": \"build\"}\n```";
        let answer = parse_answer(json_fence);
        assert_eq!(answer.text, "None.");
        assert_eq!(answer.offer, Offer::Build);

        let question = "Do you mean the bar or a panel?";
        let answer = parse_answer(question);
        assert_eq!(answer.text, question);
        assert!(answer.matches.is_empty());
    }

    #[test]
    fn the_prompt_carries_the_conversation() {
        let turns = [
            Turn {
                role: Role::User,
                text: "a clock".into(),
            },
            Turn {
                role: Role::Assistant,
                text: "Two clocks fit.".into(),
            },
        ];
        let prompt = search_prompt(
            "- a.clock — Clock [bar-widget, Widgets]: time\n",
            &turns,
            "with seconds",
        );
        assert!(prompt.starts_with(INSTRUCTIONS));
        assert!(prompt.contains("Person: a clock"));
        assert!(prompt.contains("You: Two clocks fit."));
        assert!(prompt.ends_with("with seconds\n"));
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(
            strip_ansi("\x1b[32mok\x1b[0m \x1b]8;;x\x07link\x1b]8;;\x07"),
            "ok link"
        );
    }
}
