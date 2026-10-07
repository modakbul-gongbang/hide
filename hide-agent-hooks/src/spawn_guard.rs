//! The spawn guard: a `PreToolUse` hook that refuses an agent's shell call when
//! it starts another agent through Herdr directly, and answers with the
//! `hide agent spawn --parent here ...` command that does the same thing.
//!
//! Herdr records no parent for an agent it starts, so such a child reaches the
//! Agents graph as a root with no line to its starter and no watch, and nobody
//! is woken when it stops. `hide agent spawn` writes the lineage and starts the
//! watch. The guard is a redirect, not a security boundary: it sees one shell
//! call at one layer and acts only inside a Herdr pane whose checkout Hide has
//! registered (`docs/agent-hooks.md`, The spawn guard).
//!
//! What it catches, and nothing else (PRD herdr-spawn-guard D-02):
//!
//! - `herdr agent start <name> --kind <kind> --pane <pane> [-- <agent args>]`;
//! - `herdr pane run <pane> <command>` and `herdr pane send-text <pane> <text>`
//!   whose command's first word is an agent Herdr can start.
//!
//! The command is read the way a shell reads one line: words, quotes,
//! redirections and heredocs, split at `&&`, `||`, `;`, `|`, `&`, a newline and
//! a parenthesis, so `cd x && herdr agent start ...` is caught. A launch hidden
//! one layer deeper (`bash -c`, `$(...)`, a script, an alias) is not looked for
//! and its child stays a root (PRD non-goal; engineering principle 13: a guess
//! at that layer is a second string match for the same decision).
//!
//! Every function here is pure except [`registration`], [`record_refusal`] and
//! [`unreachable`]; the hook's one decision to refuse is [`deny_output`], and
//! every other path prints nothing and lets the call run.

use std::ffi::OsStr;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use hide_platform::fs::{self, private};
use hide_platform::process::{CaptureFailureKind, OwnedChild};
use serde::Deserialize;

/// The agent kinds the pinned Herdr starts: `herdr agent start --kind`'s
/// possible values at `contracts/herdr-bundle.json`'s release (0.9.1). A kind
/// is also the canonical executable's name, so the first word of a command a
/// pane runs is matched against this list (PRD D-11). `bump-herdr.sh` moves the
/// pin; `agent_kinds_are_the_pinned_herdrs` names what to check when it does.
pub const AGENT_KINDS: [&str; 24] = [
    "pi",
    "claude",
    "codex",
    "gemini",
    "cursor",
    "devin",
    "agy",
    "cline",
    "omp",
    "mastracode",
    "opencode",
    "copilot",
    "kimi",
    "kiro",
    "droid",
    "amp",
    "grok",
    "hermes",
    "kilo",
    "qodercli",
    "qwen",
    "letta",
    "maki",
    "muse",
];

/// Cursor's CLI is `cursor-agent`; Herdr's kind for it is `cursor`.
const CURSOR_EXECUTABLE: &str = "cursor-agent";

/// The form a launch took, as the refusal log names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shape {
    AgentStart,
    PaneRun,
    SendText,
}

impl Shape {
    pub fn code(self) -> &'static str {
        match self {
            Self::AgentStart => "agent_start",
            Self::PaneRun => "pane_run",
            Self::SendText => "send_text",
        }
    }
}

/// An agent launch found in one shell call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Launch {
    pub shape: Shape,
    pub kind: &'static str,
    /// `herdr agent start <name>`'s name; the other shapes name no agent.
    pub name: Option<String>,
    /// The agent's own arguments, which `hide agent spawn` takes after `--`.
    pub args: Vec<String>,
}

/// The first launch in `command`, if any. `env` reads the hook's own
/// environment, which says what the pane's Herdr is: a call that names another
/// Herdr server, session or machine is not aimed at it, and the `hide agent
/// spawn` it would be sent to starts a child in this pane's Herdr instead.
pub fn find_launch(command: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<Launch> {
    simple_commands(command)
        .iter()
        .find_map(|words| launch_of(words, env))
}

/// The shell tool's call, as a runtime's `PreToolUse` payload carries it.
#[derive(Debug, Eq, PartialEq)]
pub struct Call {
    pub command: String,
    pub cwd: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Payload {
    tool_name: Option<String>,
    tool_input: Option<ToolInput>,
    cwd: Option<String>,
}

#[derive(Deserialize)]
struct ToolInput {
    command: Option<serde_json::Value>,
}

/// The shell call in a `PreToolUse` payload. A truncated or unreadable payload,
/// another tool and a command that is not one string yield nothing, so the call
/// runs.
pub fn read_call(payload: &[u8], truncated: bool) -> Option<Call> {
    if truncated {
        return None;
    }
    let payload: Payload = serde_json::from_slice(payload).ok()?;
    if payload.tool_name.as_deref() != Some("Bash") {
        return None;
    }
    let serde_json::Value::String(command) = payload.tool_input?.command? else {
        return None;
    };
    Some(Call {
        command,
        cwd: payload.cwd.filter(|cwd| !cwd.is_empty()).map(PathBuf::from),
    })
}

/// Whether the payload can hold a launch at all. The hook runs on every shell
/// call, so everything that is not a launch is decided by this one byte search
/// before anything is parsed, read or started (PRD B13).
pub fn may_hold_launch(payload: &[u8]) -> bool {
    payload.windows(5).any(|window| window == b"herdr")
        || payload
            .windows(14)
            .any(|window| window == b"HERDR_BIN_PATH")
}

// --- The shell line --------------------------------------------------------------

/// The simple commands of a shell line, each as its words with quoting
/// removed. A substitution (`$(...)`, `` `...` ``) stays inside its word as
/// the text it was written with: it is not parsed.
fn simple_commands(text: &str) -> Vec<Vec<String>> {
    Lexer::new(text).run()
}

struct Lexer {
    chars: Vec<char>,
    at: usize,
    commands: Vec<Vec<String>>,
    words: Vec<String>,
    word: String,
    in_word: bool,
    /// The word after a redirection operator is its target, not an argument.
    drop_next: bool,
    /// The word after `<<` names the heredoc's end; `true` strips leading tabs.
    heredoc_next: Option<bool>,
    heredocs: Vec<(String, bool)>,
    /// A quote, backtick or `$(` that the text ended inside. A shell reads such
    /// a line as a syntax error and runs none of it, so nothing in it is a launch.
    unclosed: bool,
}

impl Lexer {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            at: 0,
            commands: Vec::new(),
            words: Vec::new(),
            word: String::new(),
            in_word: false,
            drop_next: false,
            heredoc_next: None,
            heredocs: Vec::new(),
            unclosed: false,
        }
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.at + offset).copied()
    }

    fn run(mut self) -> Vec<Vec<String>> {
        while let Some(c) = self.peek(0) {
            match c {
                '\\' => self.backslash(),
                '\'' => self.single_quoted(),
                '"' => self.double_quoted(),
                '`' => self.backtick(),
                '$' if self.peek(1) == Some('(') => {
                    self.word.push('$');
                    self.in_word = true;
                    self.at += 1;
                    self.parenthesized();
                }
                ' ' | '\t' => {
                    self.end_word();
                    self.at += 1;
                }
                '\n' => {
                    self.end_command();
                    self.at += 1;
                    self.heredoc_bodies();
                }
                '#' if !self.in_word => self.comment(),
                ';' | '|' | '(' | ')' => {
                    self.end_command();
                    self.at += 1;
                    // `&&`-style doubling of the same character is one operator.
                    if matches!(c, ';' | '|') && self.peek(0) == Some(c) {
                        self.at += 1;
                    }
                }
                '&' => self.ampersand(),
                '<' | '>' => self.redirection(c),
                other => {
                    self.word.push(other);
                    self.in_word = true;
                    self.at += 1;
                }
            }
        }
        self.end_command();
        if self.unclosed {
            return Vec::new();
        }
        self.commands
    }

    fn end_word(&mut self) {
        if !self.in_word {
            return;
        }
        let word = std::mem::take(&mut self.word);
        self.in_word = false;
        if let Some(strip) = self.heredoc_next.take() {
            self.heredocs.push((word, strip));
        } else if self.drop_next {
            self.drop_next = false;
        } else {
            self.words.push(word);
        }
    }

    fn end_command(&mut self) {
        self.end_word();
        self.drop_next = false;
        self.heredoc_next = None;
        if !self.words.is_empty() {
            self.commands.push(std::mem::take(&mut self.words));
        }
    }

    fn backslash(&mut self) {
        self.at += 1;
        match self.peek(0) {
            // A backslash before a newline joins the lines.
            Some('\n') => self.at += 1,
            Some(escaped) => {
                self.word.push(escaped);
                self.in_word = true;
                self.at += 1;
            }
            None => {}
        }
    }

    fn single_quoted(&mut self) {
        self.in_word = true;
        self.at += 1;
        while let Some(c) = self.peek(0) {
            self.at += 1;
            if c == '\'' {
                return;
            }
            self.word.push(c);
        }
        self.unclosed = true;
    }

    fn double_quoted(&mut self) {
        self.in_word = true;
        self.at += 1;
        while let Some(c) = self.peek(0) {
            match c {
                '"' => {
                    self.at += 1;
                    return;
                }
                '\\' => {
                    self.at += 1;
                    match self.peek(0) {
                        Some('\n') => self.at += 1,
                        Some(escaped @ ('"' | '\\' | '$' | '`')) => {
                            self.word.push(escaped);
                            self.at += 1;
                        }
                        _ => self.word.push('\\'),
                    }
                }
                '$' if self.peek(1) == Some('(') => {
                    self.word.push('$');
                    self.at += 1;
                    self.parenthesized();
                }
                '`' => self.backtick(),
                other => {
                    self.word.push(other);
                    self.at += 1;
                }
            }
        }
        self.unclosed = true;
    }

    /// `` `...` ``, copied into the word as written.
    fn backtick(&mut self) {
        self.in_word = true;
        self.word.push('`');
        self.at += 1;
        while let Some(c) = self.peek(0) {
            self.at += 1;
            self.word.push(c);
            match c {
                '`' => return,
                '\\' => {
                    if let Some(escaped) = self.peek(0) {
                        self.word.push(escaped);
                        self.at += 1;
                    }
                }
                _ => {}
            }
        }
        self.unclosed = true;
    }

    /// `(...)` after a `$`, copied into the word as written, to its matching
    /// parenthesis; quotes inside it keep their own parentheses out of the count.
    fn parenthesized(&mut self) {
        let mut depth = 0usize;
        while let Some(c) = self.peek(0) {
            self.at += 1;
            self.word.push(c);
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                '\\' => {
                    if let Some(escaped) = self.peek(0) {
                        self.word.push(escaped);
                        self.at += 1;
                    }
                }
                '\'' | '"' => {
                    let mut closed = false;
                    while let Some(inner) = self.peek(0) {
                        self.at += 1;
                        self.word.push(inner);
                        if inner == c {
                            closed = true;
                            break;
                        }
                        if inner == '\\'
                            && c == '"'
                            && let Some(escaped) = self.peek(0)
                        {
                            self.word.push(escaped);
                            self.at += 1;
                        }
                    }
                    if !closed {
                        self.unclosed = true;
                    }
                }
                _ => {}
            }
        }
        self.unclosed = true;
    }

    fn comment(&mut self) {
        while let Some(c) = self.peek(0) {
            if c == '\n' {
                return;
            }
            self.at += 1;
        }
    }

    fn ampersand(&mut self) {
        if self.peek(1) == Some('>') {
            // `&>` and `&>>` send both streams to the next word.
            self.end_word();
            self.at += 2;
            if self.peek(0) == Some('>') {
                self.at += 1;
            }
            self.drop_next = true;
        } else {
            self.end_command();
            self.at += 1;
            if self.peek(0) == Some('&') {
                self.at += 1;
            }
        }
    }

    /// `<`, `>` and their forms. A number just before it is the descriptor, not
    /// an argument; the word after it is the target (or, for `<<`, the heredoc's
    /// end, whose body follows the line).
    fn redirection(&mut self, c: char) {
        if self.in_word && !self.word.is_empty() && self.word.chars().all(|d| d.is_ascii_digit()) {
            self.word.clear();
            self.in_word = false;
        } else {
            self.end_word();
        }
        self.at += 1;
        if c == '<' && self.peek(0) == Some('<') {
            self.at += 1;
            match self.peek(0) {
                // `<<<` is a here-string: one word, not a body.
                Some('<') => {
                    self.at += 1;
                    self.drop_next = true;
                }
                Some('-') => {
                    self.at += 1;
                    self.heredoc_next = Some(true);
                }
                _ => self.heredoc_next = Some(false),
            }
            return;
        }
        if matches!(self.peek(0), Some('>' | '&' | '|')) || (c == '<' && self.peek(0) == Some('>'))
        {
            self.at += 1;
        }
        self.drop_next = true;
    }

    /// After a newline, skips the body of each heredoc the line opened.
    fn heredoc_bodies(&mut self) {
        for (end, strip) in std::mem::take(&mut self.heredocs) {
            while self.at < self.chars.len() {
                let start = self.at;
                let stop = self.chars[start..]
                    .iter()
                    .position(|c| *c == '\n')
                    .map_or(self.chars.len(), |offset| start + offset);
                self.at = (stop + 1).min(self.chars.len());
                let line: String = self.chars[start..stop].iter().collect();
                let line = if strip {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if line == end {
                    break;
                }
            }
        }
    }
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().enumerate().all(|(index, c)| {
                c == '_' || c.is_ascii_alphabetic() || (index > 0 && c.is_ascii_digit())
            })
    })
}

/// Words a shell puts before a command without changing which command runs:
/// the keywords that open a compound command's body, and the wrappers that
/// run the next word as it is.
fn is_prefix(word: &str) -> bool {
    matches!(
        word,
        "if" | "then"
            | "elif"
            | "else"
            | "while"
            | "until"
            | "do"
            | "{"
            | "!"
            | "time"
            | "exec"
            | "command"
            | "nohup"
            | "env"
    )
}

/// The environment variables that choose which Herdr a `herdr` call talks to.
const TARGET_VARIABLES: [&str; 2] = ["HERDR_SOCKET_PATH", "HERDR_SESSION"];

/// A command's words without the `NAME=value` words a shell takes as the
/// environment of the command that follows and without the prefix words above,
/// and whether those assignments pointed `herdr` at another server than the
/// one `env` says the pane has.
fn command_words<'a>(
    words: &'a [String],
    env: &dyn Fn(&str) -> Option<String>,
) -> (&'a [String], bool) {
    let mut foreign = false;
    let mut at = 0;
    while let Some(word) = words.get(at) {
        if let Some((name, value)) = word.split_once('=')
            && is_assignment(word)
        {
            if TARGET_VARIABLES.contains(&name) && env(name).as_deref() != Some(value) {
                foreign = true;
            }
        } else if !is_prefix(word) {
            break;
        }
        at += 1;
    }
    (&words[at..], foreign)
}

fn is_herdr(word: &str) -> bool {
    matches!(word, "$HERDR_BIN_PATH" | "${HERDR_BIN_PATH}")
        || Path::new(word)
            .file_name()
            .is_some_and(|name| name == "herdr")
}

/// The kind of the agent a command word runs, matched on its file name.
fn kind_of(word: &str) -> Option<&'static str> {
    let name = Path::new(word).file_name()?.to_str()?;
    if name == CURSOR_EXECUTABLE {
        return Some("cursor");
    }
    AGENT_KINDS.into_iter().find(|kind| *kind == name)
}

fn launch_of(words: &[String], env: &dyn Fn(&str) -> Option<String>) -> Option<Launch> {
    let (words, foreign) = command_words(words, env);
    let (first, mut rest) = words.split_first()?;
    if foreign || !is_herdr(first) {
        return None;
    }
    // Herdr's own options before the subcommand; naming a session or a machine
    // aims the call away from this pane's Herdr.
    if matches!(
        rest.first().map(String::as_str),
        Some("--session" | "--machine")
    ) || rest
        .first()
        .is_some_and(|option| option.starts_with("--session=") || option.starts_with("--machine="))
    {
        return None;
    }
    let (group, verb) = (rest.first()?.as_str(), rest.get(1)?.as_str());
    let rest = &rest[2..];
    match (group, verb) {
        ("agent", "start") => agent_start(rest),
        ("pane", "run") => pane_text(Shape::PaneRun, rest),
        ("pane", "send-text") => pane_text(Shape::SendText, rest),
        _ => None,
    }
}

/// `herdr agent start <name> --kind <kind> --pane <pane> [--timeout <ms>] [-- args]`.
/// Without a known `--kind`, or with `--help`, nothing is started.
fn agent_start(words: &[String]) -> Option<Launch> {
    let mut kind = None;
    let mut name = None;
    let mut at = 0;
    while at < words.len() {
        let word = words[at].as_str();
        match word {
            "--" => {
                return Some(Launch {
                    shape: Shape::AgentStart,
                    kind: kind?,
                    name: name.map(str::to_owned),
                    args: words[at + 1..].to_vec(),
                });
            }
            "-h" | "--help" => return None,
            "--kind" => {
                kind = kind_named(words.get(at + 1)?);
                at += 1;
            }
            "--pane" | "--timeout" => at += 1,
            _ => {
                if let Some(value) = word.strip_prefix("--kind=") {
                    kind = kind_named(value);
                } else if !word.starts_with("--") && name.is_none() {
                    name = Some(word);
                }
            }
        }
        at += 1;
    }
    Some(Launch {
        shape: Shape::AgentStart,
        kind: kind?,
        name: name.map(str::to_owned),
        args: Vec::new(),
    })
}

fn kind_named(value: &str) -> Option<&'static str> {
    AGENT_KINDS.into_iter().find(|kind| *kind == value)
}

/// `herdr pane run <pane> <command>...` and `herdr pane send-text <pane> <text>`:
/// the command's first word is read, and only that.
fn pane_text(shape: Shape, words: &[String]) -> Option<Launch> {
    let (_pane, text) = words.split_first()?;
    let command: Vec<String> = match text {
        [] => return None,
        // One argument is a command line of its own.
        [line] => simple_commands(line).into_iter().next()?,
        several => several.to_vec(),
    };
    let (command, _) = command_words(&command, &|_| None);
    let (first, args) = command.split_first()?;
    Some(Launch {
        shape,
        kind: kind_of(first)?,
        name: None,
        args: args.to_vec(),
    })
}

// --- What the agent is told ------------------------------------------------------

/// A word as a shell reads it back unchanged: bare when it holds nothing a
/// shell treats specially, otherwise in single quotes.
fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | ',' | '@' | '%' | '+')
        });
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// The `hide agent spawn` command that does what `launch` does and also records
/// the parent and starts the watch (PRD D-10). What the call does not say stays
/// in angle brackets for the agent to fill: the intent always, the name when
/// the launch gave none, the repository and branch when the checkout has none.
pub fn spawn_command(launch: &Launch, repo: Option<&str>, branch: Option<&str>) -> String {
    let mut command = format!(
        "hide agent spawn --parent here --name {} --intent <intent> --kind {} --repo {} --branch {}",
        launch
            .name
            .as_deref()
            .map_or_else(|| "<name>".to_owned(), shell_word),
        launch.kind,
        repo.map_or_else(|| "<repo>".to_owned(), shell_word),
        branch.map_or_else(|| "<branch>".to_owned(), shell_word),
    );
    if !launch.args.is_empty() {
        command.push_str(" --");
        for arg in &launch.args {
            command.push(' ');
            command.push_str(&shell_word(arg));
        }
    }
    command
}

/// The reason a refused call reads, for the agent.
pub fn refusal_reason(command: &str) -> String {
    format!(
        "Not run: this call starts an agent with herdr directly, and Herdr records no parent for it, so the child would show in Hide's Agents graph with no line to you and no watch, and nobody would be woken when it stops. \
         Start it through Hide instead, filling in what is in angle brackets:\n\n{command}\n\n\
         It opens the child in its own tab, ties it to you and starts the watch. If this call held other commands, run them separately."
    )
}

/// The `PreToolUse` answer that refuses a call, for Claude Code and Codex: the
/// same envelope, observed on Claude Code 2.1.292 and codex-cli 0.160.0, with
/// the reason shown to the agent.
pub fn deny_output(reason: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

/// The repository's main root and its current branch, read from Git's own
/// files (no Git process) at `cwd`; either is `None` when `cwd` is not in a
/// repository or HEAD is detached.
pub fn checkout_facts(cwd: &Path) -> (Option<String>, Option<String>) {
    let Some(repository) = hide_project::git::discover(cwd) else {
        return (None, None);
    };
    (
        repository.main_root().to_str().map(str::to_owned),
        repository.branch(),
    )
}

// --- Asking the daemon -----------------------------------------------------------

/// What the daemon says about the caller's pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Registration {
    /// The pane sits in a checkout Hide has registered.
    Registered,
    /// The daemon answered and does not know this pane's checkout.
    NotRegistered,
    /// The daemon could not be asked: no CLI, no daemon, no answer in time.
    Unreachable(&'static str),
}

/// Asks `hide workspace bootstrap`, the daemon-owned call a session start makes
/// too: it needs no open window and succeeds only for a caller inside a
/// registered checkout. A refusal with a reason is the daemon answering; a
/// missing daemon, a timeout and an unreadable answer are not.
pub fn registration(program: &OsStr, deadline: Instant) -> Registration {
    let mut command = Command::new(program);
    command
        .args(["workspace", "bootstrap"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(mut child) = OwnedChild::spawn(&mut command) else {
        return Registration::Unreachable("cli");
    };
    let output = match child.capture_until(deadline, 16 * 1024) {
        Ok(output) => output,
        Err(failure) => {
            return Registration::Unreachable(match failure.kind {
                CaptureFailureKind::Deadline => "deadline",
                _ => "cli",
            });
        }
    };
    if output.status.success() {
        return match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            Ok(answer) if answer["ok"] == true => Registration::Registered,
            _ => Registration::Unreachable("format"),
        };
    }
    // Only the daemon's own "this caller is not in a Hide checkout" answers
    // mean the call is not Hide's to guide. Every other word, an empty one and
    // a reason a later build adds included, is a daemon that could not be
    // asked, which lets the call run and leaves a diagnostic.
    let reason = String::from_utf8_lossy(&output.stderr);
    if matches!(
        reason.trim(),
        "checkout_not_registered"
            | "caller_unavailable"
            | "pane_not_connected"
            | "pane_unavailable"
            | "pane_changed"
            | "caller_not_in_pane"
    ) {
        Registration::NotRegistered
    } else {
        Registration::Unreachable("daemon")
    }
}

// --- What is recorded ------------------------------------------------------------

const LOG_LIMIT: u64 = 256 * 1024;
const LOG_LOCK_WAIT: Duration = Duration::from_millis(100);

fn log_path(home: &Path) -> PathBuf {
    home.join(".hide")
        .join("agent-hooks")
        .join("spawn-guard.log")
}

/// One refusal, as one structured line: the pane, the agent kind, the shape the
/// call took and the runtime. The command is never written (PRD B12): it can
/// hold a prompt, a path or a secret. The file is private, holds at most
/// `LOG_LIMIT` bytes and keeps one older generation. A line that cannot be
/// written is a refusal that already happened, so nothing is retried and the
/// call is refused regardless.
pub fn record_refusal(home: &Path, runtime: &str, pane: &str, launch: &Launch) {
    let line = serde_json::json!({
        "component": "spawn_guard",
        "kind": "launch.refused",
        "runtime": runtime,
        "pane": pane,
        "agent": launch.kind,
        "shape": launch.shape.code(),
        "at_unix_ms": now_ms(),
    })
    .to_string();
    eprintln!("{line}");
    let _ = append_line(&log_path(home), &line);
}

fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("log parent unavailable"))?;
    private::create_dir_all(parent)?;
    let lock = private::open_own_file(&parent.join("spawn-guard.lock"), true)?;
    let fs::lock::Waited::Locked(_lock) =
        fs::lock::lock_file(lock, fs::lock::Mode::Exclusive, LOG_LOCK_WAIT, &|| false)?
    else {
        return Err(std::io::Error::other("log busy"));
    };
    let mut file = private::open_own_file(path, true)?;
    if file.metadata()?.len() >= LOG_LIMIT {
        drop(file);
        std::fs::rename(path, path.with_extension("log.1"))?;
        file = private::open_own_file(path, true)?;
    }
    file.seek(SeekFrom::End(0))?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()
}

/// The daemon could not be asked, so the call ran. One diagnostic per cause
/// per ten minutes, to the log file only (PRD B6; principle 10: nothing here is
/// something the operator or the agent can act on).
pub fn unreachable(home: &Path, runtime: &str, cause: &'static str) {
    if crate::delivery::claim(home, "guard") {
        let line = serde_json::json!({
            "component": "spawn_guard",
            "kind": "daemon.unreachable",
            "runtime": runtime,
            "cause": cause,
            "at_unix_ms": now_ms(),
        })
        .to_string();
        let _ = append_line(&log_path(home), &line);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

#[cfg(test)]
fn read_log(home: &Path) -> String {
    std::fs::read_to_string(log_path(home)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pane's own Herdr, as the hook's environment names it.
    fn pane_env(name: &str) -> Option<String> {
        match name {
            "HERDR_SOCKET_PATH" => Some("/run/own.sock".to_owned()),
            _ => None,
        }
    }

    fn found(command: &str) -> Launch {
        find_launch(command, &pane_env).unwrap_or_else(|| panic!("a launch in: {command}"))
    }

    fn none(command: &str) {
        assert_eq!(
            find_launch(command, &pane_env),
            None,
            "no launch in: {command}"
        );
    }

    #[test]
    fn agent_kinds_are_the_pinned_herdrs() {
        // `herdr agent start --help` lists the kinds below at the Herdr pinned
        // by `contracts/herdr-bundle.json` (0.9.1). When `bump-herdr.sh` moves
        // the pin, compare that list with this one and change both.
        let listed = "pi, claude, codex, gemini, cursor, devin, agy, cline, omp, mastracode, \
                      opencode, copilot, kimi, kiro, droid, amp, grok, hermes, kilo, qodercli, \
                      qwen, letta, maki, muse";
        assert_eq!(AGENT_KINDS.join(", "), listed);
        assert_eq!(AGENT_KINDS.len(), 24);
    }

    #[test]
    fn agent_start_is_found_with_its_name_kind_and_agent_arguments() {
        let launch = found(
            "herdr agent start set-g --kind claude --pane w1:p2 -- --model opus --add-dir '/a b'",
        );
        assert_eq!(launch.shape, Shape::AgentStart);
        assert_eq!(launch.kind, "claude");
        assert_eq!(launch.name.as_deref(), Some("set-g"));
        assert_eq!(launch.args, ["--model", "opus", "--add-dir", "/a b"]);
        // The flags may come in any order and take `=`.
        let launch = found("herdr agent start --pane=w1:p2 --kind=codex helper");
        assert_eq!(
            (launch.kind, launch.name.as_deref()),
            ("codex", Some("helper"))
        );
        assert!(launch.args.is_empty());
        let launch =
            found("/opt/herdr/bin/herdr agent start x --timeout 5000 --kind grok --pane p");
        assert_eq!(launch.kind, "grok");
    }

    #[test]
    fn pane_run_and_send_text_are_found_by_the_first_word_of_the_command() {
        let launch = found("herdr pane run w1:p2 codex --model gpt-6 'fix the tests'");
        assert_eq!(launch.shape, Shape::PaneRun);
        assert_eq!((launch.kind, launch.name), ("codex", None));
        assert_eq!(launch.args, ["--model", "gpt-6", "fix the tests"]);
        // One argument is a command line.
        let launch = found("herdr pane run w1:p2 \"claude --model opus\"");
        assert_eq!(
            (launch.kind, launch.args),
            ("claude", vec!["--model".to_owned(), "opus".to_owned()])
        );
        let launch = found("herdr pane send-text w1:p2 'FOO=1 /usr/local/bin/gemini -p hi'");
        assert_eq!(launch.shape, Shape::SendText);
        assert_eq!(
            (launch.kind, launch.args),
            ("gemini", vec!["-p".to_owned(), "hi".to_owned()])
        );
        assert_eq!(found("herdr pane run w1:p2 cursor-agent").kind, "cursor");
    }

    #[test]
    fn a_launch_is_found_in_a_chain_a_group_or_behind_an_environment() {
        for command in [
            "cd /repo && herdr agent start a --kind claude --pane p",
            "ls; herdr agent start a --kind claude --pane p",
            "false || herdr agent start a --kind claude --pane p",
            "echo hi | herdr agent start a --kind claude --pane p",
            "sleep 1 & herdr agent start a --kind claude --pane p",
            "(cd x && herdr agent start a --kind claude --pane p)",
            "cd x\nherdr agent start a --kind claude --pane p",
            "HERDR_X=1 FOO=bar herdr agent start a --kind claude --pane p",
            "\"$HERDR_BIN_PATH\" agent start a --kind claude --pane p",
            "${HERDR_BIN_PATH} agent start a --kind claude --pane p",
            "herdr agent start a --kind claude --pane p > /tmp/out 2>&1",
            "herdr agent start a --kind claude --pane p &> /tmp/out",
            "for n in a b; do herdr agent start \"$n\" --kind claude --pane p; done",
            "if true; then herdr agent start a --kind claude --pane p; fi",
            "while read n; do herdr agent start a --kind claude --pane p; done < list",
            "{ herdr agent start a --kind claude --pane p; }",
            "! herdr agent start a --kind claude --pane p",
            "time herdr agent start a --kind claude --pane p",
            "exec herdr agent start a --kind claude --pane p",
            "command herdr agent start a --kind claude --pane p",
            "nohup herdr agent start a --kind claude --pane p",
            "env FOO=1 herdr agent start a --kind claude --pane p",
            "HERDR_SOCKET_PATH=/run/own.sock herdr agent start a --kind claude --pane p",
        ] {
            assert_eq!(found(command).kind, "claude", "{command}");
        }
        // Redirections are not the agent's arguments.
        let launch = found("herdr agent start a --kind claude --pane p -- --x > /tmp/o 2>&1");
        assert_eq!(launch.args, ["--x"]);
    }

    #[test]
    fn what_is_not_a_launch_is_left_alone() {
        for command in [
            "ls -la",
            "git status",
            "herdr pane split w1:p2 --direction right",
            "herdr tab create --workspace w1",
            "herdr pane run w1:p2 cargo test",
            "herdr pane run w1:p2 'npm run dev'",
            "herdr pane run w1:p2 'cd x && claude'",
            "herdr pane run w1:p2",
            "herdr pane send-text w1:p2 'hello'",
            "herdr agent list",
            "herdr agent prompt claude-1 hello",
            "herdr agent start --help",
            "herdr agent start a --pane p",
            "herdr agent start a --kind nosuchagent --pane p",
            "herdr workspace list",
            "herdr",
            "claude --model opus",
            "echo herdr agent start a --kind claude --pane p",
            "git commit -m 'herdr agent start a --kind claude --pane p'",
            "grep -rn \"herdr pane run p claude\" docs",
            "echo \"$(herdr agent start a --kind claude --pane p)\"",
            "bash -c 'herdr agent start a --kind claude --pane p'",
            "cat <<'EOF'\nherdr agent start a --kind claude --pane p\nEOF",
            "cat <<-EOF > f\n\therdr pane run p claude\n\tEOF\nls",
            "# herdr agent start a --kind claude --pane p",
            "xherdr agent start a --kind claude --pane p",
            "herdrctl agent start a --kind claude --pane p",
            "echo do herdr agent start a --kind claude --pane p",
        ] {
            none(command);
        }
        // A launch after a heredoc's body is still found.
        assert_eq!(
            found("cat <<EOF\nbody\nEOF\nherdr agent start a --kind codex --pane p").kind,
            "codex"
        );
    }

    #[test]
    fn a_call_aimed_at_another_herdr_is_left_alone() {
        // The `hide agent spawn` this guard would offer starts the child in the
        // pane's own Herdr, so a call that names a different one is not refused.
        for command in [
            "herdr --session qa agent start a --kind claude --pane p",
            "herdr --session=qa agent start a --kind claude --pane p",
            "herdr --machine mini agent start a --kind claude --pane p",
            "HERDR_SOCKET_PATH=/tmp/qa/herdr.sock HERDR_BIN_PATH=/x/herdr herdr agent start qa --kind claude --pane w1:p1",
            "env HERDR_SOCKET_PATH=/tmp/qa.sock herdr agent start a --kind claude --pane p",
            "HERDR_SESSION=qa herdr pane run p claude",
        ] {
            none(command);
        }
        // The pane's own server named explicitly is still the pane's Herdr.
        assert_eq!(
            find_launch(
                "HERDR_BIN_PATH=/x/herdr herdr agent start a --kind claude --pane p",
                &pane_env
            )
            .map(|launch| launch.kind),
            Some("claude")
        );
    }

    #[test]
    fn text_a_shell_would_not_run_holds_no_launch() {
        // A heredoc inside `$(...)` whose body has an early `)` and an odd quote
        // reads as top-level lines to this lexer, but ends inside a quote: a
        // shell runs none of it.
        none(
            "git commit -m \"$(cat <<'EOF'\nRefuse launches\n\nSteps: 1) open the 5\" screen\nherdr agent start x --kind claude --pane p\nEOF\n)\"",
        );
        for command in [
            "herdr agent start a --kind claude --pane \"p",
            "herdr agent start a --kind claude --pane 'p",
            "herdr agent start a --kind claude --pane `p",
            "echo $(herdr agent start a --kind claude --pane p",
            "herdr agent start a --kind claude --pane p; echo \"unterminated",
        ] {
            none(command);
        }
    }

    #[test]
    fn the_payload_of_a_shell_call_is_read_and_nothing_else() {
        let call = |json: &str| read_call(json.as_bytes(), false);
        assert_eq!(
            call(r#"{"tool_name":"Bash","cwd":"/r","tool_input":{"command":"ls"}}"#),
            Some(Call {
                command: "ls".into(),
                cwd: Some(PathBuf::from("/r"))
            })
        );
        assert_eq!(
            call(r#"{"tool_name":"Edit","tool_input":{"command":"ls"}}"#),
            None
        );
        assert_eq!(
            call(r#"{"tool_name":"Bash","tool_input":{"command":["ls"]}}"#),
            None
        );
        assert_eq!(call(r#"{"tool_name":"Bash","tool_input":{}}"#), None);
        assert_eq!(call("not json"), None);
        assert_eq!(
            read_call(
                br#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#,
                true
            ),
            None
        );
        assert!(may_hold_launch(b"... herdr agent start ..."));
        assert!(may_hold_launch(b"\"$HERDR_BIN_PATH\" agent"));
        assert!(!may_hold_launch(
            br#"{"tool_name":"Bash","tool_input":{"command":"cargo test"}}"#
        ));
    }

    #[test]
    fn the_offered_command_is_filled_from_the_call() {
        let start = found("herdr agent start set-g --kind claude --pane p -- --model opus");
        assert_eq!(
            spawn_command(&start, Some("/Users/me/herdr-ide"), Some("fix/thing")),
            "hide agent spawn --parent here --name set-g --intent <intent> --kind claude --repo /Users/me/herdr-ide --branch fix/thing -- --model opus"
        );
        let run = found("herdr pane run p codex 'fix the tests'");
        assert_eq!(
            spawn_command(&run, Some("/a b"), None),
            "hide agent spawn --parent here --name <name> --intent <intent> --kind codex --repo '/a b' --branch <branch> -- 'fix the tests'"
        );
        assert_eq!(
            spawn_command(&run, None, Some("main")),
            "hide agent spawn --parent here --name <name> --intent <intent> --kind codex --repo <repo> --branch main -- 'fix the tests'"
        );
        // A single quote inside an argument survives a shell reading it back.
        let quoted = found("herdr pane run p claude \"it's\"");
        assert!(spawn_command(&quoted, None, None).ends_with("-- 'it'\\''s'"));
    }

    #[test]
    fn the_refusal_is_the_one_envelope_both_runtimes_read() {
        let output: serde_json::Value =
            serde_json::from_str(&deny_output(&refusal_reason("hide agent spawn ..."))).unwrap();
        assert_eq!(output["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        let reason = output["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap();
        assert!(reason.contains("hide agent spawn ..."));
        assert!(!reason.contains('\u{2014}'), "no em dash");
    }

    #[test]
    fn a_refusal_is_logged_without_the_command_and_the_log_is_bounded() {
        let home = tempfile::tempdir().unwrap();
        let launch =
            found("herdr agent start secret-name --kind claude --pane p -- --prompt SECRET-TEXT");
        record_refusal(home.path(), "claude-code", "w1:p2", &launch);
        let text = read_log(home.path());
        let line: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(line["pane"], "w1:p2");
        assert_eq!(line["agent"], "claude");
        assert_eq!(line["shape"], "agent_start");
        assert!(!text.contains("SECRET-TEXT") && !text.contains("secret-name"));
        assert!(private::is_private(&log_path(home.path())).unwrap());
        // Past the limit the file rotates once and keeps one older generation.
        let big = "x".repeat(LOG_LIMIT as usize);
        std::fs::write(log_path(home.path()), &big).unwrap();
        record_refusal(home.path(), "codex", "w1:p3", &launch);
        assert!(read_log(home.path()).len() < 1024);
        assert_eq!(
            std::fs::read_to_string(log_path(home.path()).with_extension("log.1")).unwrap(),
            big
        );
    }
}
