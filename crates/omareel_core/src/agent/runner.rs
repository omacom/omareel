//! Running the person's coding agent on a recording, for the editor's Polish, "Ask the editor"
//! and "Write description" (and `omareel agent`): the agent Omarchy has as the default
//! (`~/.config/omarchy/defaults/agent`, what `omarchy-default-agent` prints) runs once,
//! non-interactively, with the omareel skill and an instruction, in the bundle's directory. It
//! edits through `omareel apply`, so each change lands in an open editor through its live
//! reload, as a step the person can undo.
//!
//! Each supported agent has an [`Adapter`]: how to start it headless with its tools held to
//! omareel, and how to read the JSON events it prints ([`StreamParser`]) as [`Event`]s: its
//! messages, the commands it runs, each apply, and the end with its cost.
//!
//! The agent reads the transcript and frames, which can carry prompt injection (speech, text
//! on screen), so it is treated as untrusted:
//!
//! - Every `omareel` it runs is held to the bundle ([`super::scope`]): the runner sets
//!   `OMAREEL_AGENT_SCOPE`, and omareel then runs only describe, apply, frame (into the scratch
//!   directory) and transcribe on this bundle.
//! - Claude Code: `claude -p --output-format stream-json --verbose --restricted --tools
//!   Bash,Read --allowedTools "Bash(omareel describe:*)" "Bash(omareel apply:*)" … Read
//!   --permission-mode dontAsk`. Restricted mode ignores the person's settings files (and
//!   their broader allow rules) and keeps the file tools inside the bundle and the scratch
//!   directory; any other command is refused without asking. No MCP servers
//!   (`--strict-mcp-config`).
//! - Codex (experimental here): `codex exec --json --sandbox workspace-write
//!   --ignore-user-config --ignore-rules` with network access and web search off and no extra
//!   writable roots. The person's config.toml isn't loaded, so their MCP servers, network
//!   setting, writable roots, features and hooks don't apply (system-wide config still does).
//!   Its sandbox holds what commands *write* to the bundle and the scratch directory, with no
//!   network for them, but it does NOT restrict reads: a command can read any file the person
//!   can, and what it reads goes to OpenAI. Nor does anything hold its shell to `omareel`;
//!   `OMAREEL_AGENT_SCOPE` holds only what omareel itself does.
//!
//! The agent runs in a process group of its own ([`crate::transcript::job`]), dies with the
//! process that started it, and at the end of every run (finished, failed, timed out or
//! stopped) everything it started is killed: the group, and any process carrying this run's
//! `OMAREEL_AGENT_SCOPE`, which finds those that left the group or were re-parented away.

use super::skill::SKILL;
use crate::compat::qdir;
use crate::compat::qjson::{self, Format, Object, Value};
use crate::compat::qstandardpaths::{self, Location};
use crate::transcript::job::{self, Control, Stopped};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The longest a run may take before it is stopped.
pub const TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How an agent prints its events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StreamFormat {
    /// Claude Code's `--output-format stream-json`.
    ClaudeStreamJson,
    /// `codex exec --json`.
    CodexJson,
}

/// A coding agent omareel knows how to run headless.
#[derive(Debug, PartialEq)]
pub struct Adapter {
    /// Omarchy's name for it (`omarchy-default-agent`).
    pub id: &'static str,
    pub name: &'static str,
    pub program: &'static str,
    pub format: StreamFormat,
    /// Where it sends what it reads, for the editor to say.
    pub provider: &'static str,
    /// The environment variable that picks its account's config home (Omarchy's
    /// `omarchy-agent-account-home`).
    pub account_env: &'static str,
    /// Less contained than the others (Codex: its commands can read any file), so the editor
    /// calls it experimental.
    pub experimental: bool,
}

pub const ADAPTERS: &[Adapter] = &[
    Adapter {
        id: "claude",
        name: "Claude Code",
        program: "claude",
        format: StreamFormat::ClaudeStreamJson,
        provider: "Anthropic",
        account_env: "CLAUDE_CONFIG_DIR",
        experimental: false,
    },
    Adapter {
        id: "codex",
        name: "Codex",
        program: "codex",
        format: StreamFormat::CodexJson,
        provider: "OpenAI",
        account_env: "CODEX_HOME",
        experimental: true,
    },
];

/// The names Omarchy gives the agents it offers, for saying which one isn't supported.
const OMARCHY_AGENT_NAMES: &[(&str, &str)] = &[
    ("pi", "Pi"),
    ("omp", "Oh My Pi"),
    ("opencode", "OpenCode"),
    ("ori", "Ori"),
    ("crush", "Crush"),
    ("grok", "Grok"),
    ("openclaw", "OpenClaw"),
    ("agy", "Antigravity"),
    ("hermes", "Hermes"),
    ("copilot", "GitHub Copilot"),
    ("muse", "Muse Code"),
    ("cursor-agent", "Cursor CLI"),
    ("kimi", "Kimi"),
    ("qwen", "Qwen Code"),
    ("dsh", "DeepSeek Harness"),
    ("mcode", "MiniMax Code"),
    ("kiro-cli", "Kiro CLI"),
    ("freebuff", "Freebuff"),
    ("kilo", "Kilo"),
    ("devin", "Devin CLI"),
    ("amp", "Amp"),
    ("commandcode", "Command Code"),
];

pub fn adapter(id: &str) -> Option<&'static Adapter> {
    ADAPTERS.iter().find(|adapter| adapter.id == id)
}

/// A person-facing name for an agent id.
pub fn agent_name(id: &str) -> String {
    if let Some(adapter) = adapter(id) {
        return adapter.name.to_owned();
    }
    OMARCHY_AGENT_NAMES.iter().find(|(known, _)| *known == id).map_or_else(|| id.to_owned(), |(_, name)| (*name).to_owned())
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// The agent the person chose in Omarchy (Setup › Defaults › Agent), or `$OMAREEL_AGENT`.
pub fn default_agent_id() -> Option<String> {
    if let Some(id) = env("OMAREEL_AGENT") {
        return Some(id);
    }
    let file = format!("{}/.config/omarchy/defaults/agent", qdir::home_path());
    let text = std::fs::read_to_string(file).ok()?;
    text.lines().next().map(str::trim).filter(|id| !id.is_empty()).map(str::to_owned)
}

/// Finds an agent's command: `$OMAREEL_AGENT_PROGRAM`, else on `$PATH`, else where Omarchy
/// installs agents (mise's wrappers and shims, `~/.local/bin`), for an editor started without
/// the login shell's `$PATH`.
pub fn find_program(adapter: &Adapter) -> Option<PathBuf> {
    if let Some(program) = env("OMAREEL_AGENT_PROGRAM") {
        return Some(PathBuf::from(program));
    }
    let found = qstandardpaths::find_executable(adapter.program);
    if !found.is_empty() {
        return Some(PathBuf::from(found));
    }
    let home = qdir::home_path();
    let data = env("XDG_DATA_HOME").unwrap_or_else(|| format!("{home}/.local/share"));
    let mise = env("MISE_DATA_DIR").unwrap_or_else(|| format!("{data}/mise"));
    [format!("{mise}/command-wrappers/bin"), format!("{mise}/shims"), format!("{home}/.local/bin")]
        .iter()
        .map(|dir| Path::new(dir).join(adapter.program))
        .find(|path| { let path = path.to_string_lossy().into_owned(); let info = qdir::FileInfo::new(path.as_str()); info.is_file() && info.is_executable() })
}

/// Which agent would run, for the editor to show before anything runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    Ready { adapter: &'static Adapter, program: PathBuf },
    /// No default agent is set in Omarchy.
    NoneChosen,
    /// The default agent is one omareel can't run headless (yet).
    Unsupported { id: String },
    NotInstalled { adapter: &'static Adapter },
}

impl Choice {
    /// The agent the person chose, and whether it can run.
    pub fn find() -> Choice {
        match default_agent_id() {
            None => Choice::NoneChosen,
            Some(id) => match adapter(&id) {
                None => Choice::Unsupported { id },
                Some(adapter) => match find_program(adapter) {
                    Some(program) => Choice::Ready { adapter, program },
                    None => Choice::NotInstalled { adapter },
                },
            },
        }
    }

    /// Why it can't run, for the person; empty when it can.
    pub fn problem(&self) -> String {
        let supported = ADAPTERS.iter().map(|adapter| adapter.name).collect::<Vec<_>>().join(" and ");
        match self {
            Choice::Ready { .. } => String::new(),
            Choice::NoneChosen => {
                format!("No coding agent is set up. Choose one in Omarchy (Setup › Defaults › Agent); omareel works with {supported}.")
            }
            Choice::Unsupported { id } => format!(
                "Omarchy's default agent is {}, which omareel can't run on its own yet. It works with {supported}: choose one in Omarchy (Setup › Defaults › Agent).",
                agent_name(id)
            ),
            Choice::NotInstalled { adapter } => {
                format!("{} is Omarchy's default agent, but its `{}` command isn't installed.", adapter.name, adapter.program)
            }
        }
    }
}

/// What the agent is asked to do.
#[derive(Clone, Debug, PartialEq)]
pub enum TaskKind {
    /// Tighten the recording: trims, idle stretches and pauses, zooms, keystrokes.
    Polish,
    /// The person's own request.
    Ask(String),
    /// chapters.json: chapters, a title and a description.
    Chapters,
}

impl TaskKind {
    pub fn name(&self) -> &'static str {
        match self {
            TaskKind::Polish => "polish",
            TaskKind::Ask(_) => "ask",
            TaskKind::Chapters => "chapters",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Task {
    /// The bundle's absolute path; the agent's working directory.
    pub bundle: String,
    pub kind: TaskKind,
    /// Where the agent may write (frames to look at): a directory of this run's own
    /// ([`Task::new`] makes it), which the [`Runner`] removes when its next run starts or
    /// when it goes (the editor closing, `omareel agent` ending). The run leaves the agent's
    /// output there too (stream.jsonl, stderr.txt).
    pub scratch: PathBuf,
    /// The omareel binary the agent's `omareel` runs, put first on its `$PATH`, so it is the
    /// version of the editor that started it.
    pub omareel: Option<PathBuf>,
    /// A model to ask the agent for (`$OMAREEL_AGENT_MODEL`); its own default otherwise.
    pub model: Option<String>,
}

impl Task {
    /// A task with a new scratch directory of its own under `~/.cache/omareel/agent/`, this
    /// omareel binary, and `$OMAREEL_AGENT_MODEL`.
    pub fn new(bundle: &str, kind: TaskKind) -> Result<Task, String> {
        Task::in_cache(bundle, kind, &qstandardpaths::writable_location(Location::GenericCache))
    }

    /// [`Task::new`] with the scratch directory under `<cache>/omareel/agent/`.
    pub fn in_cache(bundle: &str, kind: TaskKind, cache: &str) -> Result<Task, String> {
        let name: String = qdir::FileInfo::new(bundle)
            .file_name()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
            .collect();
        let parent = PathBuf::from(format!("{cache}/omareel/agent"));
        std::fs::create_dir_all(&parent).map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        remove_stale_scratches(&parent);
        Ok(Task {
            bundle: bundle.to_owned(),
            kind,
            scratch: make_temp_dir(&parent, &name)?,
            omareel: std::env::current_exe().ok(),
            model: env("OMAREEL_AGENT_MODEL"),
        })
    }
}

/// A new directory `<parent>/<name>-XXXXXX` (mkdtemp: unique, only for this user).
fn make_temp_dir(parent: &Path, name: &str) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStrExt;
    let template = parent.join(format!("{name}-XXXXXX"));
    let mut bytes = template.as_os_str().as_bytes().to_vec();
    bytes.push(0);
    // SAFETY: mkdtemp fills in the NUL-terminated template in place.
    let made = unsafe { libc::mkdtemp(bytes.as_mut_ptr().cast()) };
    if made.is_null() {
        return Err(format!("cannot create {}: {}", template.display(), std::io::Error::last_os_error()));
    }
    bytes.pop();
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(&bytes)))
}

/// Scratch directories a day old: left by an editor that didn't get to remove its own.
fn remove_stale_scratches(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else { return };
    for entry in entries.flatten() {
        let old = entry.metadata().and_then(|metadata| metadata.modified()).ok().and_then(|modified| modified.elapsed().ok());
        if old.is_some_and(|age| age > Duration::from_secs(24 * 60 * 60)) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

const RULES: &str = "\
# Working inside the Omareel editor

The person started you from the Omareel editor with a click, and is watching the editor while you
work. They can't answer questions: don't ask any. Decide, and say what you chose at the end.

- The recording is the bundle at {bundle}, which is your working directory (`.`). Work only on it,
  and only through `omareel` commands: `omareel describe .`, `omareel frame`,
  `omareel transcribe .`, and `omareel apply . '[...]'` with the operations as one
  single-quoted JSON argument (no heredoc, pipe, or file: only plain `omareel` commands are
  allowed here). Run one command per call, without `&&`, `;` or redirections. Don't export,
  install anything, or edit files by hand.
- Each `omareel apply` shows up in the editor at once, as one step the person can undo. Put
  related operations in one apply (all the cuts together, say), so each step means something.
  Use `\"timeline\": \"source\"` for batches of cuts.
- Write images only to {scratch}, for example
  `omareel frame . --count 9 --sheet -o {scratch}/sheet-1.jpg`, then view them with your
  file-reading tool.
- End with two or three plain sentences for the person: what you changed, in output times they
  can find on the timeline.
";

const POLISH: &str = "\
Polish this recording so it is tight and easy to follow:

1. Run `omareel describe .`.
2. Trim the start and the end: dead time before the first action, and reaching for the stop
   button at the end.
3. Cut long idle stretches and pauses, leaving a beat on each side. If there is speech, use
   `trim-pauses`. When what happens on screen matters (a build, a page loading), speed the
   stretch up instead of cutting it.
4. Check the zooms: keep the automatic ones that help, fix or remove the ones that don't, and
   zoom where something small matters. After big cuts, consider `regenerate-zooms`.
5. Turn on keystrokes if keyboard shortcuts are part of what is shown.
6. Look at a contact sheet of the result, and fix what looks wrong.

Keep the person's content: never cut speech or the moments where results appear. Don't change
the background, the style, the music or the export settings.
";

const ASK: &str = "\
The person asked:

\"{request}\"

Do that, and only that. Read `omareel describe .` first. If omareel's operations can't do it,
change nothing and say why.
";

const CHAPTERS: &str = "\
Write chapters, a title and a short description for this video, for YouTube.

1. Run `omareel describe .`. Find where the topic changes from the transcript if there is one
   (no transcript? run `omareel transcribe .`; if no speech engine is set up, go on without),
   from markers if describe lists any, and from the activity (clicks, shortcuts, idle
   stretches). Look at a contact sheet to see what is on screen.
2. Chapters: 3 to 8 of them, the first at the very start of the video, each at least 10 s
   long, each title 2 to 6 words, in the recording's language. A video under a minute may have
   none.
3. A title under 70 characters, plain, no clickbait, and a description of one to three
   sentences on what the video shows.
4. Save them with one apply:
   `{\"op\": \"set-chapters\", \"timeline\": \"source\", \"title\": \"...\", \"description\": \"...\",
   \"chapters\": [{\"at\": SOURCE_SECONDS, \"title\": \"...\"}, ...]}`.
   Use source times from describe, so later edits don't move the chapters.

Make no other edit.
";

/// The instructions that go with the skill: how to work inside the editor.
pub fn system_prompt(task: &Task) -> String {
    let rules = RULES.replace("{bundle}", &task.bundle).replace("{scratch}", &task.scratch.to_string_lossy());
    format!("{SKILL}\n\n{rules}")
}

/// The task itself.
pub fn user_prompt(task: &Task) -> String {
    match &task.kind {
        TaskKind::Polish => POLISH.to_owned(),
        TaskKind::Ask(request) => ASK.replace("{request}", request.trim()),
        TaskKind::Chapters => CHAPTERS.to_owned(),
    }
}

/// The agent's command line for `task` (without its environment; see [`command`]).
pub fn arguments(adapter: &Adapter, task: &Task) -> Vec<String> {
    let scratch = task.scratch.to_string_lossy().into_owned();
    let mut args: Vec<String> = Vec::new();
    match adapter.format {
        StreamFormat::ClaudeStreamJson => {
            args.extend(
                [
                    "-p",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--restricted",
                    "--tools",
                    "Bash,Read",
                    // Only the commands omareel itself allows in the agent's scope.
                    "--allowedTools",
                    "Bash(omareel describe:*)",
                    "Bash(omareel apply:*)",
                    "Bash(omareel frame:*)",
                    "Bash(omareel transcribe:*)",
                    "Bash(omareel help)",
                    "Read",
                    "--permission-mode",
                    "dontAsk",
                    "--strict-mcp-config",
                    "--no-session-persistence",
                    "--add-dir",
                ]
                .map(String::from),
            );
            args.push(scratch);
            args.push("--append-system-prompt".into());
            args.push(system_prompt(task));
            if let Some(model) = &task.model {
                args.extend(["--model".into(), model.clone()]);
            }
            args.push("--".into());
            args.push(user_prompt(task));
        }
        StreamFormat::CodexJson => {
            args.extend(
                [
                    "exec",
                    "--json",
                    "--skip-git-repo-check",
                    "--ephemeral",
                    "--sandbox",
                    "workspace-write",
                    // Not the person's config.toml (their MCP servers, network setting,
                    // writable roots, features, hooks) nor their exec rules; and network, web
                    // search and extra writable roots off whatever system config says.
                    "--ignore-user-config",
                    "--ignore-rules",
                    "-c",
                    "approval_policy=\"never\"",
                    "-c",
                    "sandbox_workspace_write.network_access=false",
                    "-c",
                    "sandbox_workspace_write.writable_roots=[]",
                    "-c",
                    "web_search=\"disabled\"",
                    "--cd",
                ]
                .map(String::from),
            );
            args.push(task.bundle.clone());
            args.extend(["--add-dir".into(), scratch]);
            if let Some(model) = &task.model {
                args.extend(["--model".into(), model.clone()]);
            }
            // Codex has no system prompt of its own to add to: the skill leads the prompt.
            args.push("--".into());
            args.push(format!("{}\n\n# Task\n\n{}", system_prompt(task), user_prompt(task)));
        }
    }
    args
}

/// Links the omareel binary into the scratch directory's `bin/`.
fn prepare(task: &Task) -> Result<(), String> {
    if task.bundle.contains('\n') || task.scratch.to_string_lossy().contains('\n') {
        return Err("the recording's path has a line break in it, which the agent can't be held to".into());
    }
    let bin = task.scratch.join("bin");
    std::fs::create_dir_all(&bin).map_err(|error| format!("cannot create {}: {error}", bin.display()))?;
    if let Some(omareel) = &task.omareel {
        let link = bin.join("omareel");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(omareel, &link).map_err(|error| format!("cannot link omareel into {}: {error}", bin.display()))?;
    }
    Ok(())
}

/// The account home Omarchy has selected for the agent (`omarchy-agent-account-home`), unless
/// the environment already names one. Empty for the main account.
fn account_home(adapter: &Adapter) -> Option<String> {
    if env(adapter.account_env).is_some() {
        return None;
    }
    let program = qstandardpaths::find_executable("omarchy-agent-account-home");
    if program.is_empty() {
        return None;
    }
    let mut command = Command::new(program);
    command.arg(adapter.id);
    let output = job::run(command, Some(Duration::from_secs(5)), &Control::default()).ok()?;
    let home = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.code == 0 && !home.is_empty()).then_some(home)
}

/// The agent's command: in the bundle, with omareel first on `$PATH`.
pub fn command(adapter: &Adapter, program: &Path, task: &Task) -> Command {
    let mut command = Command::new(program);
    command.args(arguments(adapter, task)).current_dir(&task.bundle);
    let bin = task.scratch.join("bin");
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    command.env("PATH", format!("{}:{path}", bin.display()));
    command.env("OMAREEL_BUNDLE", &task.bundle);
    command.env("OMAREEL_AGENT_TASK", task.kind.name());
    // Holds every omareel the agent runs to this bundle; also marks its processes for the
    // sweep at the end ([`sweep`]).
    command.env(super::scope::SCOPE_ENV, super::scope::value(&task.bundle, &task.scratch));
    if let Some(home) = account_home(adapter) {
        command.env(adapter.account_env, home);
    }
    command
}

/// What the agent did, as the editor shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The agent is up; `model` when it says.
    Started { agent: String, model: String },
    /// Something the agent wrote for the person.
    Message(String),
    /// A command it ran (other than an apply).
    Command(String),
    /// An `omareel apply`: what it did, the video's length after it, or why it failed.
    Applied { ok: bool, summary: String, output_duration: Option<f64>, error: String },
    /// An image it looked at (frames, contact sheets).
    Looked(String),
    /// A tool call refused or failed, or the agent's own error.
    Problem(String),
    /// The end of the run: its last words, and what it cost when the agent says.
    Finished { ok: bool, summary: String, cost_usd: Option<f64>, duration_ms: Option<f64>, turns: Option<i64>, tokens: Option<i64> },
}

impl Event {
    pub fn kind(&self) -> &'static str {
        match self {
            Event::Started { .. } => "started",
            Event::Message(_) => "message",
            Event::Command(_) => "command",
            Event::Applied { .. } => "apply",
            Event::Looked(_) => "image",
            Event::Problem(_) => "problem",
            Event::Finished { .. } => "finished",
        }
    }

    /// The line the editor shows.
    pub fn text(&self) -> String {
        match self {
            Event::Started { agent, model } if model.is_empty() => format!("{agent} started"),
            Event::Started { agent, model } => format!("{agent} started ({model})"),
            Event::Message(text) | Event::Command(text) | Event::Looked(text) | Event::Problem(text) => text.clone(),
            Event::Applied { ok: true, summary, output_duration, .. } => match output_duration {
                Some(duration) => format!("{summary} · now {}", short_duration(*duration)),
                None => summary.clone(),
            },
            Event::Applied { ok: false, error, .. } => format!("Apply failed: {error}"),
            Event::Finished { ok, summary, .. } => {
                if !summary.is_empty() {
                    summary.clone()
                } else if *ok {
                    "Done".into()
                } else {
                    "The agent stopped with an error".into()
                }
            }
        }
    }

    /// One JSON object, as `omareel agent` prints it.
    pub fn to_json(&self) -> Object {
        let mut object = Object::from_iter([("type", Value::from(self.kind())), ("text", self.text().into())]);
        let optional = |object: &mut Object, key: &str, value: Option<f64>| {
            if let Some(value) = value {
                object.insert(key, value);
            }
        };
        match self {
            Event::Started { agent, model } => {
                object.insert("agent", agent.as_str());
                object.insert("model", model.as_str());
            }
            Event::Applied { ok, output_duration, .. } => {
                object.insert("ok", *ok);
                optional(&mut object, "output_duration", *output_duration);
            }
            Event::Looked(path) => object.insert("path", path.as_str()),
            Event::Finished { ok, cost_usd, duration_ms, turns, tokens, .. } => {
                object.insert("ok", *ok);
                optional(&mut object, "cost_usd", *cost_usd);
                optional(&mut object, "duration_ms", *duration_ms);
                optional(&mut object, "turns", turns.map(|t| t as f64));
                optional(&mut object, "tokens", tokens.map(|t| t as f64));
            }
            _ => {}
        }
        object
    }
}

/// `1:02.5`, `42.5 s`.
fn short_duration(seconds: f64) -> String {
    if seconds >= 60.0 {
        let minutes = (seconds / 60.0).floor();
        format!("{}:{:04.1}", minutes as i64, seconds - minutes * 60.0)
    } else {
        format!("{seconds:.1} s")
    }
}

/// Whether a shell command runs `omareel apply` for real (a dry run changes nothing).
pub fn is_apply(command: &str) -> bool {
    command.split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | '\'' | '"'))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| (pair[0] == "omareel" || pair[0].ends_with("/omareel")) && pair[1] == "apply")
        && !command.contains("--dry-run")
}

/// The command as the person would type it: a shell wrapper (`bash -lc '…'`) taken off, and
/// a heredoc's body left out.
pub fn display_command(command: &str) -> String {
    let mut text = command.trim();
    for wrapper in ["/bin/bash -lc ", "bash -lc ", "/bin/sh -c ", "sh -c ", "/usr/bin/bash -lc ", "/bin/zsh -lc ", "zsh -lc "] {
        if let Some(rest) = text.strip_prefix(wrapper) {
            text = rest.trim();
            let quoted = (text.starts_with('\'') && text.ends_with('\'')) || (text.starts_with('"') && text.ends_with('"'));
            if quoted && text.len() >= 2 {
                text = &text[1..text.len() - 1];
            }
            break;
        }
    }
    let first = text.lines().next().unwrap_or_default();
    if text.contains('\n') { format!("{first} …") } else { first.to_owned() }
}

/// The images an `omareel frame` command wrote (`-o FILE`, or FILE-1, FILE-2… for several),
/// those that exist, with relative paths taken from `cwd`.
pub fn rendered_images(command: &str, cwd: &str) -> Vec<String> {
    let words: Vec<&str> = command.split_whitespace().map(|word| word.trim_matches(|c| c == '\'' || c == '"')).collect();
    let frame = words.windows(2).any(|pair| (pair[0] == "omareel" || pair[0].ends_with("/omareel")) && pair[1] == "frame");
    let Some(output) = words.windows(2).find(|pair| frame && (pair[0] == "-o" || pair[0] == "--output")).map(|pair| pair[1]) else {
        return Vec::new();
    };
    let path = if output.starts_with('/') || cwd.is_empty() { output.to_owned() } else { qdir::file_path(cwd, output) };
    if Path::new(&path).is_file() {
        return vec![path];
    }
    let (stem, extension) = path.rsplit_once('.').unwrap_or((&path, ""));
    (1..=24)
        .map(|n| if path.contains("%d") { path.replace("%d", &n.to_string()) } else { format!("{stem}-{n}.{extension}") })
        .take_while(|candidate| Path::new(candidate).is_file())
        .collect()
}

/// What an apply's JSON report says: a summary of its operations, the new length, or the error.
pub fn apply_report(output: &str, failed: bool) -> Event {
    let report = output
        .find('{')
        .zip(output.rfind('}'))
        .filter(|(start, end)| start < end)
        .and_then(|(start, end)| qjson::object_from_json(output[start..=end].as_bytes()));
    let Some(report) = report else {
        let error = output.trim().lines().last().unwrap_or("no report").to_owned();
        return Event::Applied { ok: false, summary: String::new(), output_duration: None, error };
    };
    if !report.value("ok").to_bool(!failed) {
        return Event::Applied {
            ok: false,
            summary: String::new(),
            output_duration: None,
            error: report.value("error").to_string_or("unknown error"),
        };
    }
    let mut counts: Vec<(String, usize)> = Vec::new();
    for result in report.value("results").as_array().unwrap_or(&[]) {
        let op = result.get("op").to_string_or("?");
        match counts.iter_mut().find(|(name, _)| *name == op) {
            Some((_, count)) => *count += 1,
            None => counts.push((op, 1)),
        }
    }
    let ops: Vec<String> = counts.iter().map(|(op, count)| if *count > 1 { format!("{op} ×{count}") } else { op.clone() }).collect();
    let saved = report.value("saved").to_bool(true);
    let summary = if ops.is_empty() { "Applied nothing".to_owned() } else { format!("Applied {}", ops.join(", ")) };
    Event::Applied {
        ok: true,
        summary: if saved { summary } else { format!("{summary} (dry run)") },
        output_duration: report.get("output_duration").map(|value| value.to_double(0.0)),
        error: String::new(),
    }
}

/// A tool call waiting for its result.
struct Pending {
    /// The command, or for a Read the file.
    command: String,
    apply: bool,
    read: bool,
}

/// Reads an agent's JSON lines as they come, in whatever chunks.
pub struct StreamParser {
    format: StreamFormat,
    agent: String,
    buffer: Vec<u8>,
    pending: HashMap<String, Pending>,
    /// The last thing the agent wrote, for Codex, whose end doesn't repeat it.
    last_message: String,
    finished: bool,
    /// The agent's working directory, for relative paths.
    cwd: String,
    /// Images already reported.
    images: Vec<String>,
}

fn is_image(path: &str) -> bool {
    let lower = path.to_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif"].iter().any(|ext| lower.ends_with(ext))
}

/// A tool result's text: a string, or a list of text blocks.
fn content_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().filter_map(|item| item.get("text").as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn first_line(text: &str, limit: usize) -> String {
    let line = text.trim().lines().next().unwrap_or_default();
    if line.chars().count() > limit { format!("{}…", line.chars().take(limit).collect::<String>()) } else { line.to_owned() }
}

impl StreamParser {
    pub fn new(adapter: &Adapter) -> StreamParser {
        StreamParser {
            format: adapter.format,
            agent: adapter.name.to_owned(),
            buffer: Vec::new(),
            pending: HashMap::new(),
            last_message: String::new(),
            finished: false,
            cwd: String::new(),
            images: Vec::new(),
        }
    }

    /// Reads relative paths from `cwd` (the bundle).
    pub fn in_directory(mut self, cwd: &str) -> StreamParser {
        self.cwd = cwd.to_owned();
        self
    }

    /// [`Event::Looked`] for each image not reported yet.
    fn looked(&mut self, paths: Vec<String>, events: &mut Vec<Event>) {
        for path in paths {
            if !self.images.contains(&path) {
                self.images.push(path.clone());
                events.push(Event::Looked(path));
            }
        }
    }

    /// Whether the agent reported its end.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// The events in `chunk` and what came before it, for each whole line.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Event> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(end) = self.buffer.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            events.extend(self.line(&line));
        }
        events
    }

    /// The events of a last line without its newline.
    pub fn finish(&mut self) -> Vec<Event> {
        let line = std::mem::take(&mut self.buffer);
        self.line(&line)
    }

    fn line(&mut self, line: &[u8]) -> Vec<Event> {
        let text = String::from_utf8_lossy(line);
        let text = text.trim();
        if !text.starts_with('{') {
            return Vec::new();
        }
        let Some(object) = qjson::object_from_json(text.as_bytes()) else { return Vec::new() };
        match self.format {
            StreamFormat::ClaudeStreamJson => self.claude(&object),
            StreamFormat::CodexJson => self.codex(&object),
        }
    }

    fn claude(&mut self, object: &Object) -> Vec<Event> {
        let mut events = Vec::new();
        match object.value("type").as_str().unwrap_or_default() {
            "system" if object.value("subtype").as_str() == Some("init") => events.push(Event::Started {
                agent: self.agent.clone(),
                model: object.value("model").to_string_or(""),
            }),
            "assistant" => {
                // A subagent's messages carry their parent's id; only the agent's own count.
                if !object.value("parent_tool_use_id").as_str().unwrap_or_default().is_empty() {
                    return events;
                }
                for block in object.value("message").get("content").as_array().unwrap_or(&[]) {
                    match block.get("type").as_str().unwrap_or_default() {
                        "text" => {
                            let text = block.get("text").to_string_or("").trim().to_owned();
                            if !text.is_empty() {
                                self.last_message = text.clone();
                                events.push(Event::Message(text));
                            }
                        }
                        "tool_use" => {
                            let id = block.get("id").to_string_or("");
                            let input = block.get("input");
                            match block.get("name").as_str().unwrap_or_default() {
                                "Bash" => {
                                    let command = input.get("command").to_string_or("");
                                    let apply = is_apply(&command);
                                    if !apply {
                                        events.push(Event::Command(display_command(&command)));
                                    }
                                    self.pending.insert(id, Pending { command, apply, read: false });
                                }
                                "Read" => {
                                    let path = input.get("file_path").to_string_or("");
                                    if !is_image(&path) {
                                        events.push(Event::Command(format!("Read {path}")));
                                    }
                                    self.pending.insert(id, Pending { command: path, apply: false, read: true });
                                }
                                other => events.push(Event::Command(other.to_owned())),
                            }
                        }
                        _ => {}
                    }
                }
            }
            "user" => {
                for block in object.value("message").get("content").as_array().unwrap_or(&[]) {
                    if block.get("type").as_str() != Some("tool_result") {
                        continue;
                    }
                    let Some(pending) = self.pending.remove(&block.get("tool_use_id").to_string_or("")) else { continue };
                    let failed = block.get("is_error").to_bool(false);
                    let output = content_text(block.get("content"));
                    if pending.apply {
                        events.push(apply_report(&output, failed));
                    } else if failed {
                        let what = if pending.read { format!("Read {}", pending.command) } else { display_command(&pending.command) };
                        events.push(Event::Problem(format!("{what}: {}", first_line(&output, 200))));
                    } else if pending.read && is_image(&pending.command) {
                        let path = if pending.command.starts_with('/') { pending.command } else { qdir::file_path(&self.cwd, &pending.command) };
                        self.looked(vec![path], &mut events);
                    } else if !pending.read {
                        let images = rendered_images(&pending.command, &self.cwd);
                        self.looked(images, &mut events);
                    }
                }
            }
            "result" => {
                self.finished = true;
                let ok = !object.value("is_error").to_bool(false) && object.value("subtype").as_str() == Some("success");
                let number = |key: &str| object.get(key).filter(|value| value.is_double()).map(|value| value.to_double(0.0));
                let usage = object.value("usage");
                let tokens = ["input_tokens", "output_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"]
                    .iter()
                    .map(|key| usage.get(key).to_integer(0))
                    .sum::<i64>();
                let summary = match object.value("result").as_str() {
                    Some(text) if !text.trim().is_empty() => text.trim().to_owned(),
                    _ if !ok => object
                        .value("errors")
                        .as_array()
                        .and_then(|errors| errors.first())
                        .map(|error| error.to_string_or(""))
                        .unwrap_or_else(|| object.value("subtype").to_string_or("error")),
                    _ => self.last_message.clone(),
                };
                events.push(Event::Finished {
                    ok,
                    summary,
                    cost_usd: number("total_cost_usd"),
                    duration_ms: number("duration_ms"),
                    turns: object.get("num_turns").map(|value| value.to_integer(0)),
                    tokens: (tokens > 0).then_some(tokens),
                });
            }
            _ => {}
        }
        events
    }

    fn codex(&mut self, object: &Object) -> Vec<Event> {
        let mut events = Vec::new();
        let item = object.value("item");
        let item_type = item.get("type").as_str().unwrap_or_default();
        let id = item.get("id").to_string_or("");
        match object.value("type").as_str().unwrap_or_default() {
            "thread.started" => events.push(Event::Started { agent: self.agent.clone(), model: String::new() }),
            "item.started" if item_type == "command_execution" => {
                let command = item.get("command").to_string_or("");
                let apply = is_apply(&command);
                if !apply {
                    events.push(Event::Command(display_command(&command)));
                }
                self.pending.insert(id, Pending { command, apply, read: false });
            }
            "item.completed" => match item_type {
                "agent_message" => {
                    let text = item.get("text").to_string_or("").trim().to_owned();
                    if !text.is_empty() {
                        self.last_message = text.clone();
                        events.push(Event::Message(text));
                    }
                }
                "command_execution" => {
                    let command = item.get("command").to_string_or("");
                    let pending = self.pending.remove(&id).unwrap_or_else(|| Pending { apply: is_apply(&command), command, read: false });
                    let code = item.get("exit_code");
                    let failed = item.get("status").as_str() == Some("failed") || (code.is_double() && code.to_int(0) != 0);
                    let output = item.get("aggregated_output").to_string_or("");
                    if pending.apply {
                        events.push(apply_report(&output, failed));
                    } else if failed {
                        events.push(Event::Problem(format!("{}: {}", display_command(&pending.command), first_line(&output, 200))));
                    } else {
                        let images = rendered_images(&display_command(&pending.command), &self.cwd);
                        self.looked(images, &mut events);
                    }
                }
                "error" => events.push(Event::Problem(item.get("message").to_string_or("error"))),
                _ => {}
            },
            "turn.completed" => {
                self.finished = true;
                let usage = object.value("usage");
                let tokens = usage.get("input_tokens").to_integer(0) + usage.get("output_tokens").to_integer(0);
                events.push(Event::Finished {
                    ok: true,
                    summary: self.last_message.clone(),
                    cost_usd: None,
                    duration_ms: None,
                    turns: None,
                    tokens: (tokens > 0).then_some(tokens),
                });
            }
            "turn.failed" => {
                self.finished = true;
                events.push(Event::Finished {
                    ok: false,
                    summary: object.value("error").get("message").to_string_or("the turn failed"),
                    cost_usd: None,
                    duration_ms: None,
                    turns: None,
                    tokens: None,
                });
            }
            "error" => events.push(Event::Problem(object.value("message").to_string_or("error"))),
            _ => {}
        }
        events
    }
}

/// How a run ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub ok: bool,
    pub summary: String,
    pub applies: usize,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<f64>,
}

/// Starting, following and stopping runs, one at a time, from any thread.
#[derive(Clone)]
pub struct Runner {
    control: Control,
    current: Arc<Current>,
    timeout: Duration,
}

impl Default for Runner {
    fn default() -> Runner {
        Runner { control: Control::default(), current: Arc::default(), timeout: TIMEOUT }
    }
}

/// The run going on or last ended: its scope (marking its processes) and its scratch
/// directory, removed when the next run starts or the last clone of the runner goes.
#[derive(Default)]
struct Current(Mutex<Option<(String, PathBuf)>>);

impl Current {
    fn replace(&self, next: Option<(String, PathBuf)>) {
        let previous = std::mem::replace(&mut *self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()), next);
        if let Some((_, scratch)) = previous {
            let _ = std::fs::remove_dir_all(scratch);
        }
    }

    fn scope(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().map(|(scope, _)| scope.clone())
    }
}

impl Drop for Current {
    fn drop(&mut self) {
        self.replace(None);
    }
}

impl Runner {
    pub fn new() -> Runner {
        Runner::default()
    }

    /// A runner that stops runs after `timeout` instead of [`TIMEOUT`].
    pub fn with_timeout(timeout: Duration) -> Runner {
        Runner { timeout, ..Runner::default() }
    }

    /// Runs the agent to its end (on the calling thread), handing each event to `on_event` as
    /// it comes. An error says why it couldn't start, was stopped, or ended without a result.
    pub fn run(&self, adapter: &Adapter, program: &Path, task: &Task, on_event: impl FnMut(Event)) -> Result<Outcome, String> {
        let scope = super::scope::value(&task.bundle, &task.scratch);
        self.current.replace(Some((scope.clone(), task.scratch.clone())));
        let result = self.run_task(adapter, program, task, on_event);
        // However it ended, nothing the agent started outlives the run.
        sweep(&scope);
        result
    }

    fn run_task(&self, adapter: &Adapter, program: &Path, task: &Task, mut on_event: impl FnMut(Event)) -> Result<Outcome, String> {
        prepare(task)?;
        let mut parser = StreamParser::new(adapter).in_directory(&task.bundle);
        let mut outcome = Outcome::default();
        let mut handle = |event: Event, outcome: &mut Outcome| {
            match &event {
                Event::Applied { ok: true, .. } => outcome.applies += 1,
                Event::Finished { ok, summary, cost_usd, duration_ms, .. } => {
                    outcome.ok = *ok;
                    outcome.summary = summary.clone();
                    outcome.cost_usd = *cost_usd;
                    outcome.duration_ms = *duration_ms;
                }
                _ => {}
            }
            on_event(event);
        };
        // The agent's own words, kept for looking into a run afterwards.
        let mut transcript = std::fs::File::create(task.scratch.join("stream.jsonl")).ok();
        let result = job::run_streaming(command(adapter, program, task), Some(self.timeout), &self.control, |chunk| {
            if let Some(file) = transcript.as_mut() {
                let _ = std::io::Write::write_all(file, chunk);
            }
            for event in parser.feed(chunk) {
                handle(event, &mut outcome);
            }
            true
        });
        for event in parser.finish() {
            handle(event, &mut outcome);
        }
        let output = match result {
            Ok(output) => output,
            Err(Stopped::Cancelled) => return Err("Stopped".into()),
            Err(Stopped::TimedOut) if self.timeout.as_secs() >= 60 => return Err(format!("Stopped after {} minutes", self.timeout.as_secs() / 60)),
            Err(Stopped::TimedOut) => return Err(format!("Stopped after {} seconds", self.timeout.as_secs_f64())),
            Err(Stopped::Held) => return Err("Stopped".into()),
            Err(Stopped::Failed(error)) => return Err(error),
        };
        let _ = std::fs::write(task.scratch.join("stderr.txt"), &output.stderr);
        if !parser.finished() {
            let detail = output.stderr.trim().lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
            return Err(if detail.is_empty() {
                format!("{} ended without finishing (exit status {})", adapter.name, output.code)
            } else {
                format!("{} ended without finishing: {detail}", adapter.name)
            });
        }
        Ok(outcome)
    }

    /// Stops the run now: kills the agent's process group, and anything it started that left
    /// the group (a tool's own session, a daemon).
    pub fn stop(&self) {
        let groups = self.control.children.groups();
        let mut doomed = Vec::new();
        for group in &groups {
            doomed.extend(descendants(*group));
        }
        self.control.stop();
        for pid in doomed {
            // SAFETY: kill with a pid we just found among the agent's descendants.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
        if let Some(scope) = self.current.scope() {
            sweep(&scope);
        }
    }

    /// Ready for another run after [`Runner::stop`] (the scratch directory it keeps stays
    /// until the next run starts).
    pub fn reset(&mut self) {
        self.control = Control::default();
    }

    pub fn stopped(&self) -> bool {
        self.control.cancelled()
    }
}

/// Kills every process of this user's that has `OMAREEL_AGENT_SCOPE=scope` in its
/// environment: what a run started, also what left its process group or was re-parented away
/// from it (each run's scope names a scratch directory of its own). A process that cleared
/// its environment isn't found.
pub fn sweep(scope: &str) -> usize {
    let marker = format!("{}={scope}", super::scope::SCOPE_ENV).into_bytes();
    let own = std::process::id() as i32;
    let mut killed = 0;
    let Ok(entries) = std::fs::read_dir("/proc") else { return 0 };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<i32>().ok()) else { continue };
        if pid == own {
            continue;
        }
        // Readable only for this user's processes.
        let Ok(environ) = std::fs::read(entry.path().join("environ")) else { continue };
        if environ.split(|&byte| byte == 0).any(|variable| variable == marker.as_slice()) {
            // SAFETY: kill with a pid whose environment names this run.
            unsafe { libc::kill(pid, libc::SIGKILL) };
            killed += 1;
        }
    }
    killed
}

/// Every process below `pid`, from /proc.
pub fn descendants(pid: i32) -> Vec<i32> {
    let mut parents: HashMap<i32, Vec<i32>> = HashMap::new();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Some(child) = entry.file_name().to_str().and_then(|name| name.parse::<i32>().ok()) else { continue };
            let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else { continue };
            // The fields after the command's parenthesised name: state, then the parent.
            let Some(rest) = stat.rfind(')').map(|end| &stat[end + 1..]) else { continue };
            if let Some(parent) = rest.split_whitespace().nth(1).and_then(|field| field.parse::<i32>().ok()) {
                parents.entry(parent).or_default().push(child);
            }
        }
    }
    let mut result = Vec::new();
    let mut queue = vec![pid];
    while let Some(next) = queue.pop() {
        for &child in parents.get(&next).map(Vec::as_slice).unwrap_or(&[]) {
            result.push(child);
            queue.push(child);
        }
    }
    result
}

/// `omareel agent <bundle> polish|chapters|ask TEXT...`: runs the default agent, printing each
/// event as a JSON line; the exit status.
pub fn agent_command(arguments: &[String]) -> i32 {
    use std::io::Write;
    let usage = |error: &str| {
        let _ = std::io::stderr().write_all(crate::cli::usage_text(error).as_bytes());
        crate::cli::usage_status(error)
    };
    let Some(bundle) = arguments.first() else { return usage("agent requires a bundle and a task") };
    let kind = match arguments.get(1).map(String::as_str) {
        Some("polish") if arguments.len() == 2 => TaskKind::Polish,
        Some("chapters") if arguments.len() == 2 => TaskKind::Chapters,
        Some("ask") if arguments.len() > 2 => TaskKind::Ask(arguments[2..].join(" ")),
        _ => return usage("agent takes polish, chapters, or ask followed by a request"),
    };
    let bundle = qdir::absolute_path(bundle);
    if !Path::new(&qdir::file_path(&bundle, "screen.mp4")).is_file() {
        let _ = writeln!(std::io::stderr(), "omareel: {bundle} is not a recording (no screen.mp4)");
        return 1;
    }
    let choice = Choice::find();
    let Choice::Ready { adapter, program } = choice else {
        let _ = writeln!(std::io::stderr(), "omareel: {}", choice.problem());
        return 1;
    };
    let runner = Runner::new();
    stop_on_interrupt(&runner);
    let task = match Task::new(&bundle, kind) {
        Ok(task) => task,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "omareel: {error}");
            return 1;
        }
    };
    let print = |object: Object| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{}", object.to_json(Format::Compact));
        let _ = out.flush();
    };
    match runner.run(adapter, &program, &task, |event| print(event.to_json())) {
        Ok(outcome) if outcome.ok => 0,
        Ok(_) => 1,
        Err(error) => {
            print(Object::from_iter([("type", Value::from("error")), ("text", error.into())]));
            1
        }
    }
}

static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn interrupted(_: libc::c_int) {
    INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Ctrl+C reaches only omareel (the agent has a process group of its own): pass it on.
fn stop_on_interrupt(runner: &Runner) {
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGINT, interrupted as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, interrupted as *const () as libc::sighandler_t);
    }
    let runner = runner.clone();
    std::thread::spawn(move || {
        while !INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
        }
        runner.stop();
    });
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
