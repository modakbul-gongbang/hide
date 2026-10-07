//! The agent adapters: which coding agents Hide knows how to reach, and what
//! it puts where each one reads it (issue #517).
//!
//! One data row per agent declares the programs it is found by, where its
//! skills live, whether it has hooks and
//! the official page every one of those answers comes from. The content an
//! agent reads is the same for all of them and is read from the `hide` binary
//! at run time (`hide browser help`), so a new agent is a row here and a
//! fixture, never a new text (engineering rule 2, 7).
//!
//! Three things a row can turn on for an agent the operator switched on:
//!
//! - the `hide-browser` skill stub, in the folder the agent reads
//!   ([`SkillDir`]); the folder is shared by every agent that reads it, so
//!   the stub stays while any of them is on;
//! - a hook: Claude Code and Codex keep their kit parts
//!   ([`HookSupport::Part`]), the other agents with documented command hooks
//!   get the SessionStart guidance hook ([`HookSupport::Guidance`]);
//! - nothing else. What Hide never writes is listed in `docs/agent-hooks.md`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use hide_agent_hooks::guidance::GuidanceAgent;
use serde::{Deserialize, Serialize};

use crate::{ComponentId, ComponentState, KitTarget};

/// The operating systems a skill folder is documented for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Os {
    Macos,
    Linux,
    Windows,
}

impl Os {
    pub const CURRENT: Os = if cfg!(target_os = "macos") {
        Os::Macos
    } else if cfg!(windows) {
        Os::Windows
    } else {
        Os::Linux
    };

    const ALL: &'static [Os] = &[Os::Macos, Os::Linux, Os::Windows];
}

/// A folder an agent reads skills from, under the account's home.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SkillDir {
    /// `~/.agents/skills`, which most agents read.
    Shared,
    /// `~/.claude/skills`: Claude Code does not read the shared folder.
    Claude,
}

impl SkillDir {
    pub const ALL: [SkillDir; 2] = [Self::Shared, Self::Claude];

    /// The stable name the record carries.
    pub fn code(self) -> &'static str {
        match self {
            Self::Shared => "agents",
            Self::Claude => "claude",
        }
    }

    fn relative(self) -> [&'static str; 2] {
        match self {
            Self::Shared => [".agents", "skills"],
            Self::Claude => [".claude", "skills"],
        }
    }

    /// `~/<folder>/skills`.
    pub fn path(self, home: &Path) -> PathBuf {
        let [folder, skills] = self.relative();
        home.join(folder).join(skills)
    }

    /// Whether the folder can be written without creating the agent's own
    /// folder: the shared folder is Hide's to create, an agent's own is not,
    /// because its presence is how the hook code tells that the agent's
    /// settings are there (`hide_agent_hooks`' runtime-absent answer), so a
    /// pass that made it would let the next pass write a hook this one did
    /// not, and a second apply would not be a no-op.
    pub(crate) fn writable(self, home: &Path) -> bool {
        let [folder, _] = self.relative();
        self == Self::Shared || home.join(folder).is_dir()
    }

    fn skill_file(self, home: &Path) -> PathBuf {
        skill_file_in(&self.path(home))
    }
}

fn skill_folder_in(root: &Path) -> PathBuf {
    root.join(SKILL_NAME)
}

fn skill_file_in(root: &Path) -> PathBuf {
    skill_folder_in(root).join("SKILL.md")
}

/// What Hide writes into an agent's hook configuration, if anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookSupport {
    /// Claude Code and Codex: the six-event hook that is also one kit part.
    Part(ComponentId),
    /// The SessionStart guidance hook for an agent with documented command
    /// hooks (`hide_agent_hooks::guidance`).
    Guidance(GuidanceAgent),
    /// No hook: the agent gets the skill only. `docs/agent-hooks.md` carries
    /// the reason and the page that supports it.
    None,
}

/// The integration Herdr itself ships for an agent
/// (`herdr integration install <name>`, [`crate::herdr_integration`]).
#[derive(Clone, Copy, Debug)]
pub struct HerdrIntegration {
    /// The target name Herdr's CLI knows the agent by.
    pub name: &'static str,
    /// The agent's own configuration folder under the account's home, which
    /// Herdr's install refuses to create; Hide does not either.
    pub folder: &'static [&'static str],
}

/// One agent Hide knows.
#[derive(Clone, Copy, Debug)]
pub struct AgentAdapter {
    /// The stable id the record, the wire and the Settings rows carry.
    pub id: &'static str,
    /// The name the operator reads (a product name, never translated).
    pub label: &'static str,
    /// The program names the vendor's install documentation gives the CLI;
    /// the agent is installed on a machine when one of them is found
    /// ([`Detection`]). A folder the agent creates does not count: an editor
    /// makes `~/.cursor` without the `cursor-agent` CLI, and a CLI that was
    /// removed leaves its folder behind.
    pub executables: &'static [&'static str],
    /// The folder the agent reads skills from.
    pub skill_dir: SkillDir,
    /// The systems the agent's documentation confirms that folder on.
    pub skill_os: &'static [Os],
    pub hook: HookSupport,
    /// Herdr's integration for the agent. Every supported agent has one: an
    /// agent the pinned Herdr ships no integration for is not supported
    /// (`docs/agent-hooks.md`, Which agents Hide supports).
    pub herdr: HerdrIntegration,
    /// Whether the agent is on without the operator choosing, as Claude Code
    /// and Codex have been since their hooks became part of the kit.
    pub default_on: bool,
    /// Whether the core rings the doorbell for this agent: only kinds whose
    /// permission and selection menus were observed to read `blocked` in
    /// Herdr are targets (`docs/delivery.md`, Safe intake). The core's own list
    /// decides it; `runtime::tests::agent_features` holds this to that list.
    pub bell: bool,
    /// Whether Hide reads this agent's own session files, which sleep, fork,
    /// starting it from Hide's screen and conversation-based titles all need.
    /// Only Claude Code and Codex have such a reader (PRD settings-cleanup
    /// D-10); the gates in `herdr-core` that name the same two agents are held
    /// to this by `runtime::tests::agent_features`.
    pub session_reader: bool,
    /// The official page the row's answers come from.
    pub doc_url: &'static str,
}

const ALL_OS: &[Os] = Os::ALL;
const UNIX_OS: &[Os] = &[Os::Macos, Os::Linux];

/// Every agent, in the order Settings lists them.
pub const ADAPTERS: &[AgentAdapter] = &[
    AgentAdapter {
        id: "claude-code",
        label: "Claude Code",
        executables: &["claude"],
        skill_dir: SkillDir::Claude,
        skill_os: ALL_OS,
        hook: HookSupport::Part(ComponentId::ClaudeCodeHook),
        herdr: HerdrIntegration {
            name: "claude",
            folder: &[".claude"],
        },
        default_on: true,
        bell: true,
        session_reader: true,
        doc_url: "https://code.claude.com/docs/en/skills",
    },
    AgentAdapter {
        id: "codex",
        label: "Codex",
        executables: &["codex"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::Part(ComponentId::CodexHook),
        herdr: HerdrIntegration {
            name: "codex",
            folder: &[".codex"],
        },
        default_on: true,
        bell: true,
        session_reader: true,
        doc_url: "https://learn.chatgpt.com/docs/build-skills",
    },
    AgentAdapter {
        id: "grok",
        label: "Grok",
        executables: &["grok"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        herdr: HerdrIntegration {
            name: "grok",
            folder: &[".grok"],
        },
        default_on: false,
        bell: false,
        session_reader: false,
        doc_url: "https://docs.x.ai/build/features/skills-plugins-marketplaces",
    },
    AgentAdapter {
        id: "opencode",
        label: "OpenCode",
        executables: &["opencode"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        herdr: HerdrIntegration {
            name: "opencode",
            folder: &[".config", "opencode"],
        },
        default_on: false,
        bell: false,
        session_reader: false,
        doc_url: "https://opencode.ai/docs/skills/",
    },
    AgentAdapter {
        id: "pi",
        label: "Pi",
        executables: &["pi"],
        skill_dir: SkillDir::Shared,
        skill_os: ALL_OS,
        hook: HookSupport::None,
        herdr: HerdrIntegration {
            name: "pi",
            folder: &[".pi", "agent"],
        },
        default_on: false,
        bell: false,
        session_reader: false,
        doc_url: "https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/skills.md",
    },
    AgentAdapter {
        id: "omp",
        label: "omp",
        executables: &["omp"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        herdr: HerdrIntegration {
            name: "omp",
            folder: &[".omp", "agent"],
        },
        default_on: false,
        bell: false,
        session_reader: false,
        doc_url: "https://omp.sh/docs/skills",
    },
    AgentAdapter {
        id: "cursor",
        label: "Cursor",
        executables: &["cursor-agent"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::Guidance(GuidanceAgent::Cursor),
        herdr: HerdrIntegration {
            name: "cursor",
            folder: &[".cursor"],
        },
        default_on: false,
        bell: false,
        session_reader: false,
        doc_url: "https://cursor.com/docs/context/skills",
    },
];

/// The skill's name, and the folder it lives in.
pub const SKILL_NAME: &str = "hide-browser";

/// The marker line of the stub, which proves Hide wrote it and says which
/// text it was. A file without it is the operator's and is never touched.
const SKILL_MARKER_NAME: &str = "hide-skill";

/// Raise it when the stub's text changes, so an older stub is replaced.
pub const SKILL_VERSION: u32 = 1;

pub fn adapter(id: &str) -> Option<&'static AgentAdapter> {
    ADAPTERS.iter().find(|adapter| adapter.id == id)
}

/// The id of the agent whose hook is the kit part `part`, when there is one.
pub fn agent_of_part(part: ComponentId) -> Option<&'static str> {
    adapter_of_part(part).map(|adapter| adapter.id)
}

/// The agent whose hook is the kit part `part`, when there is one.
pub(crate) fn adapter_of_part(part: ComponentId) -> Option<&'static AgentAdapter> {
    ADAPTERS
        .iter()
        .find(|adapter| adapter.hook == HookSupport::Part(part))
}

/// One thing Hide does for an agent, as the Partial popover lists it (PRD
/// settings-cleanup D-10, B18). The ids are stable names the shell localizes;
/// no sentence lives here.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    /// The `hide-browser` skill stub in the folder the agent reads.
    Skill,
    /// The session-start guidance: the purpose instruction and the live
    /// Workspace commands, written by a hook.
    Guidance,
    /// Letters taken in when the operator submits a prompt.
    Letters,
    /// The doorbell: Hide types its one-line bell into an idle pane so the
    /// pending letters are read without the operator typing (PRD
    /// agent-neutral-doorbell). The core's bell target list decides it.
    Bell,
    /// Project Memory put into the session.
    Memory,
    /// The count of subagents the session spawned.
    Subagents,
    /// Refusing a shell call that starts an agent through Herdr directly and
    /// answering with the `hide agent spawn` command that keeps the lineage
    /// (PRD herdr-spawn-guard). The hook's `PreToolUse` entry carries it.
    SpawnGuard,
    /// Herdr's own integration, which gives the session identity and an exact
    /// status. Every supported agent has it.
    HerdrIntegration,
    /// Putting the agent to sleep and waking it.
    Sleep,
    Fork,
    /// Starting the agent from Hide's own screen.
    Start,
    /// Titles made from the conversation.
    Titles,
}

impl Feature {
    pub const ALL: [Feature; 12] = [
        Self::Skill,
        Self::Guidance,
        Self::Letters,
        Self::Bell,
        Self::Memory,
        Self::Subagents,
        Self::SpawnGuard,
        Self::HerdrIntegration,
        Self::Sleep,
        Self::Fork,
        Self::Start,
        Self::Titles,
    ];
}

impl AgentAdapter {
    /// Whether the hook entries Herdr's integration writes for this agent are
    /// covered by Hide's trust for it: only Codex asks the operator to review
    /// a hook before it runs, and Hide records that trust for it
    /// (`hide_agent_hooks::codex_trust`, PRD codex-herdr-hook-trust).
    pub(crate) fn trusts_herdr_hook(&self) -> bool {
        self.hook == HookSupport::Part(ComponentId::CodexHook)
    }

    /// Whether Hide does `feature` for this agent in this build. Every answer
    /// is read off a field of the row that also drives the behavior, so the
    /// popover cannot say more than the kit installs: the hooks from
    /// [`HookSupport`], the integration from [`HerdrIntegration`], the four
    /// that need a session reader from `session_reader`.
    pub fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Skill => true,
            Feature::Guidance => !matches!(self.hook, HookSupport::None),
            // The six-event hook is the one that carries `PreToolUse`, so an agent
            // has the guard exactly when it has that hook.
            Feature::Letters | Feature::Memory | Feature::Subagents | Feature::SpawnGuard => {
                matches!(self.hook, HookSupport::Part(_))
            }
            Feature::Bell => self.bell,
            Feature::HerdrIntegration => true,
            Feature::Sleep | Feature::Fork | Feature::Start | Feature::Titles => {
                self.session_reader
            }
        }
    }

    /// Whether Hide does only some of what it does for Claude Code, so the
    /// row wears the Partial chip whether or not the agent is on.
    pub fn partial(&self) -> bool {
        Feature::ALL
            .into_iter()
            .any(|feature| !self.supports(feature))
    }

    /// Whether the agent's documentation confirms its skill folder here.
    pub fn skill_supported(&self) -> bool {
        self.skill_os.contains(&Os::CURRENT)
    }

    /// Whether Hide writes this agent's hook on this system.
    pub fn hook_supported(&self) -> bool {
        match self.hook {
            HookSupport::Part(_) => true,
            HookSupport::Guidance(agent) => agent.supported_here().is_ok(),
            HookSupport::None => false,
        }
    }
}

/// What one pass found out about the agents on a machine: each adapter's
/// program, looked for once on one search. Every decision of the pass reads
/// this instead of searching again.
pub(crate) struct Detection {
    rows: Vec<(&'static str, Option<PathBuf>)>,
}

impl Detection {
    pub(crate) fn probe(target: &KitTarget) -> Self {
        let path = search_path(target);
        let rows = ADAPTERS
            .iter()
            .map(|adapter| {
                let program = adapter
                    .executables
                    .iter()
                    .find_map(|name| hide_platform::host::find_program(&path, name));
                (adapter.id, program)
            })
            .collect();
        Self { rows }
    }

    /// Whether the agent is installed here: one of its programs is found.
    pub(crate) fn installed(&self, adapter: &AgentAdapter) -> bool {
        self.executable(adapter).is_some()
    }

    pub(crate) fn executable(&self, adapter: &AgentAdapter) -> Option<&Path> {
        self.rows
            .iter()
            .find(|(id, _)| *id == adapter.id)
            .and_then(|(_, program)| program.as_deref())
    }
}

/// Where the agents' programs are looked for: the folders the account's
/// login shell puts on its `PATH`, then the daemon's own `PATH` and the
/// usual install folders (`hide_platform::programs`). A join fails
/// only for a folder whose name holds the separator, which no search can
/// hold, so then nothing is found.
fn search_path(target: &KitTarget) -> OsString {
    // A test machine's own agents must not decide what a fixture home finds,
    // so under test the search is the fixture's `~/.local/bin` and what the
    // fixture's stand-in shell answers, never this process's `PATH`.
    #[cfg(test)]
    {
        let shell = hide_platform::programs::login_shell_path(
            &target.home,
            target.login_shell.as_deref(),
            &target.stop,
        );
        let folders = shell
            .iter()
            .flat_map(std::env::split_paths)
            .chain([target.home.join(".local/bin")]);
        std::env::join_paths(folders).unwrap_or_default()
    }
    #[cfg(not(test))]
    {
        hide_platform::programs::search_path(
            &target.home,
            target.login_shell.as_deref(),
            &target.stop,
        )
        .unwrap_or_default()
    }
}

/// The account's login shell: `$SHELL` on macOS and Linux, which launchd
/// gives an app and sshd gives an exec channel. Windows has none to ask; a
/// process there is given the account's own `Path`. A Unix process started
/// without `SHELL` asks none, and the log says so.
pub(crate) fn login_shell() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    hide_platform::host::default_shell()
        .inspect_err(|error| eprintln!("kit.login_shell_missing kind={:?}", error.kind()))
        .ok()
}

// --- The skill stub ------------------------------------------------------------

/// The text of the stub. It points at the binary and carries no usage, so it
/// stays right as `hide browser help` changes.
pub(crate) fn skill_text() -> String {
    format!(
        "---\nname: {SKILL_NAME}\ndescription: Read and drive a browser display inside Hide. Use when a task involves a web page, a local app in a browser tab, or checking what a page shows.\n---\n\n<!-- {SKILL_MARKER_NAME}@{SKILL_VERSION}: written by Hide; remove it from Settings, Agents -->\n\nRun `hide browser help` and follow what it prints. It explains how to open a page, read it by refs, check a change with `--diff`, and what to do when a command fails.\n"
    )
}

/// The version in the stub's marker line. The marker counts only where Hide
/// puts it, as the first line after the front matter and in Hide's own form:
/// a file that merely mentions `hide-skill@` somewhere is the operator's.
fn skill_marker_version(text: &str) -> Option<u32> {
    let body = text.strip_prefix("---\n")?;
    let (_, after) = body.split_once("\n---\n")?;
    let line = after.lines().find(|line| !line.trim().is_empty())?.trim();
    let rest = line
        .strip_prefix("<!-- ")?
        .strip_prefix(SKILL_MARKER_NAME)?
        .strip_prefix('@')?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    rest[digits.len()..]
        .starts_with(':')
        .then_some(())
        .filter(|()| line.ends_with("-->"))?;
    digits.parse().ok()
}

/// What one skill folder holds, before the kit decides anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SkillObserved {
    Current,
    /// Hide's stub of an older version: a pass replaces it.
    Stale,
    /// Hide's marker at this version (or a newer one) over text that is not
    /// the stub's: the operator edited it. A pass leaves it as it is and only
    /// Reinstall puts Hide's text back.
    Edited,
    Missing,
    /// A file there that Hide did not write; it is left as it is.
    Foreign,
    Unreadable(String),
}

pub(crate) fn observe_skill(dir: SkillDir, home: &Path) -> SkillObserved {
    observe_skill_in(&dir.path(home))
}

/// What the skills folder `root` holds of Hide's stub.
pub(crate) fn observe_skill_in(root: &Path) -> SkillObserved {
    let path = skill_file_in(root);
    match std::fs::read_to_string(&path) {
        Ok(text) => match skill_marker_version(&text) {
            Some(SKILL_VERSION) if text == skill_text() => SkillObserved::Current,
            Some(version) if version < SKILL_VERSION => SkillObserved::Stale,
            Some(_) => SkillObserved::Edited,
            None => SkillObserved::Foreign,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => SkillObserved::Missing,
        Err(error) => {
            SkillObserved::Unreadable(format!("{} could not be read: {error}", path.display()))
        }
    }
}

pub(crate) fn install_skill(dir: SkillDir, home: &Path) -> Result<(), String> {
    let path = dir.skill_file(home);
    crate::write_atomically(
        &path,
        skill_text().as_bytes(),
        hide_platform::fs::Access::KeepOrPrivate,
    )
}

/// Takes the stub out when it is Hide's, and the folder with it when nothing
/// else is in it.
pub(crate) fn remove_skill(dir: SkillDir, home: &Path) -> Result<bool, String> {
    remove_skill_in(&dir.path(home))
}

/// [`remove_skill`] for any skills folder, which the retirement of an agent
/// Hide no longer knows reaches by its path.
pub(crate) fn remove_skill_in(root: &Path) -> Result<bool, String> {
    match observe_skill_in(root) {
        // An edited stub carries the operator's changes, so it stays.
        SkillObserved::Missing | SkillObserved::Foreign | SkillObserved::Edited => Ok(false),
        SkillObserved::Unreadable(reason) => Err(reason),
        SkillObserved::Current | SkillObserved::Stale => {
            let path = skill_file_in(root);
            std::fs::remove_file(&path)
                .map_err(|error| format!("{} could not be removed: {error}", path.display()))?;
            // Only an empty folder goes; anything else in it is the
            // operator's.
            let _ = std::fs::remove_dir(skill_folder_in(root));
            Ok(true)
        }
    }
}

pub(crate) fn skill_location(dir: SkillDir, home: &Path) -> String {
    dir.skill_file(home).display().to_string()
}

/// One piece of an agent (its skill, its hook) as the report states it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PieceReport {
    pub state: ComponentState,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
}

/// Why an agent has no switch on this machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Installed, and at least one of its pieces (the skill, the hook) works
    /// on this system: the switch works.
    Available,
    /// None of its programs is found on this machine.
    NotInstalled,
    /// Installed, but neither the skill folder nor the hook is documented for
    /// this system, so there is nothing for the switch to do.
    UnsupportedSystem,
}

/// What the switch can do for an agent: it needs the agent installed and at
/// least one piece that works on this system.
pub(crate) fn availability(installed: bool, skill_here: bool, hook_here: bool) -> Availability {
    if !installed {
        Availability::NotInstalled
    } else if !skill_here && !hook_here {
        Availability::UnsupportedSystem
    } else {
        Availability::Available
    }
}

/// One agent as Settings shows it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentReport {
    pub id: String,
    pub label: String,
    pub availability: Availability,
    /// Whether the agent is on: switched on by the operator, or on by
    /// default and not switched off.
    pub enabled: bool,
    /// The machine's record holds the operator's own choice for this agent,
    /// on or off. An agent that is on only by default has none, so a shell
    /// can tell one the operator switched on, whose program then went away
    /// and which keeps its switch, from a default that never had a program.
    /// Absent in a report from a build that predates it, which reads as no
    /// choice.
    #[serde(default)]
    pub chosen: bool,
    pub skill: PieceReport,
    /// `None` for an agent with no hook (skill only).
    pub hook: Option<PieceReport>,
    /// Herdr's integration for the agent. This build fills it for every
    /// agent; `None` only in a report from a device helper whose build
    /// predates the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub herdr: Option<PieceReport>,
    pub doc_url: String,
}

impl AgentReport {
    /// Whether Reinstall would change something for this agent: only an
    /// agent that is on has pieces to repair.
    pub fn needs_attention(&self) -> bool {
        self.enabled
            && (self.skill.state.needs_attention()
                || self
                    .hook
                    .as_ref()
                    .is_some_and(|hook| hook.state.needs_attention())
                || self
                    .herdr
                    .as_ref()
                    .is_some_and(|herdr| herdr.state.needs_attention()))
    }
}
