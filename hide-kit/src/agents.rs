//! The agent adapters: which coding agents Hide knows how to reach, and what
//! it puts where each one reads it (issue #517).
//!
//! One data row per agent declares how it is detected, where its skills
//! live, whether it has hooks, the oldest version the hook needs and the
//! official page every one of those answers comes from. The content an agent
//! reads is the same for all of them and is read from the `hide` binary at
//! run time (`hide browser help`), so a new agent is a row here and a
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

use std::path::{Path, PathBuf};

use hide_agent_hooks::guidance::GuidanceAgent;
use serde::{Deserialize, Serialize};

use crate::{ComponentId, ComponentState};

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
    /// `~/.kiro/skills`.
    Kiro,
    /// `~/.qwen/skills`.
    Qwen,
    /// `~/.cline/skills`.
    Cline,
}

impl SkillDir {
    pub const ALL: [SkillDir; 5] = [
        Self::Shared,
        Self::Claude,
        Self::Kiro,
        Self::Qwen,
        Self::Cline,
    ];

    /// The stable name the record carries.
    pub fn code(self) -> &'static str {
        match self {
            Self::Shared => "agents",
            Self::Claude => "claude",
            Self::Kiro => "kiro",
            Self::Qwen => "qwen",
            Self::Cline => "cline",
        }
    }

    fn relative(self) -> [&'static str; 2] {
        match self {
            Self::Shared => [".agents", "skills"],
            Self::Claude => [".claude", "skills"],
            Self::Kiro => [".kiro", "skills"],
            Self::Qwen => [".qwen", "skills"],
            Self::Cline => [".cline", "skills"],
        }
    }

    /// `~/<folder>/skills`.
    pub fn path(self, home: &Path) -> PathBuf {
        let [folder, skills] = self.relative();
        home.join(folder).join(skills)
    }

    /// Whether the folder can be written without creating the agent's own
    /// folder: the shared folder is Hide's to create, an agent's own is not,
    /// because its presence is how other parts of the kit see the agent
    /// (a pass that made it would change what the next pass finds).
    pub(crate) fn writable(self, home: &Path) -> bool {
        let [folder, _] = self.relative();
        self == Self::Shared || home.join(folder).is_dir()
    }

    /// The skill folder's own folder, `~/.<agent>/skills/hide-browser`.
    fn skill_folder(self, home: &Path) -> PathBuf {
        self.path(home).join(SKILL_NAME)
    }

    fn skill_file(self, home: &Path) -> PathBuf {
        self.skill_folder(home).join("SKILL.md")
    }
}

/// What Hide writes into an agent's hook configuration, if anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookSupport {
    /// Claude Code and Codex: the five-event hook that is also one kit part.
    Part(ComponentId),
    /// The SessionStart guidance hook for an agent with documented command
    /// hooks (`hide_agent_hooks::guidance`).
    Guidance(GuidanceAgent),
    /// No hook: the agent gets the skill only. `docs/agent-hooks.md` carries
    /// the reason and the page that supports it.
    None,
}

/// One agent Hide knows.
#[derive(Clone, Copy, Debug)]
pub struct AgentAdapter {
    /// The stable id the record, the wire and the Settings rows carry.
    pub id: &'static str,
    /// The name the operator reads (a product name, never translated).
    pub label: &'static str,
    /// Program names looked for on the login `PATH` and the usual install
    /// folders; empty when the name is too generic to mean this agent.
    pub executables: &'static [&'static str],
    /// Folders under the home the agent creates; any one present means it is
    /// set up here.
    pub home_markers: &'static [&'static str],
    /// The folder the agent reads skills from.
    pub skill_dir: SkillDir,
    /// The systems the agent's documentation confirms that folder on.
    pub skill_os: &'static [Os],
    pub hook: HookSupport,
    /// The oldest agent version whose documentation has the hook Hide
    /// writes; `None` when the documentation names none.
    pub min_version: Option<&'static str>,
    /// Whether the agent is on without the operator choosing, as Claude Code
    /// and Codex have been since their hooks became part of the kit.
    pub default_on: bool,
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
        home_markers: &[".claude"],
        skill_dir: SkillDir::Claude,
        skill_os: ALL_OS,
        hook: HookSupport::Part(ComponentId::ClaudeCodeHook),
        min_version: Some("2.1.278"),
        default_on: true,
        doc_url: "https://code.claude.com/docs/en/skills",
    },
    AgentAdapter {
        id: "codex",
        label: "Codex",
        executables: &["codex"],
        home_markers: &[".codex"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::Part(ComponentId::CodexHook),
        min_version: Some("0.155.1"),
        default_on: true,
        doc_url: "https://learn.chatgpt.com/docs/build-skills",
    },
    AgentAdapter {
        id: "opencode",
        label: "OpenCode",
        executables: &["opencode"],
        home_markers: &[".config/opencode"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://opencode.ai/docs/skills/",
    },
    AgentAdapter {
        id: "gemini-cli",
        label: "Gemini CLI",
        executables: &["gemini"],
        home_markers: &[".gemini"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::Guidance(GuidanceAgent::Gemini),
        min_version: None,
        default_on: false,
        doc_url: "https://geminicli.com/docs/cli/skills/",
    },
    AgentAdapter {
        id: "cursor",
        label: "Cursor",
        executables: &["cursor-agent"],
        home_markers: &[".cursor"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://cursor.com/docs/context/skills",
    },
    AgentAdapter {
        id: "copilot-cli",
        label: "GitHub Copilot CLI",
        executables: &["copilot"],
        home_markers: &[".copilot"],
        skill_dir: SkillDir::Shared,
        skill_os: ALL_OS,
        hook: HookSupport::Guidance(GuidanceAgent::Copilot),
        min_version: None,
        default_on: false,
        doc_url: "https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-skills",
    },
    AgentAdapter {
        id: "amp",
        label: "Amp",
        executables: &["amp"],
        home_markers: &[".config/amp"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://ampcode.com/docs/customize/skills",
    },
    AgentAdapter {
        id: "factory-droid",
        label: "Factory Droid",
        executables: &["droid"],
        home_markers: &[".factory"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::Guidance(GuidanceAgent::Droid),
        min_version: None,
        default_on: false,
        doc_url: "https://docs.factory.com/cli/configuration/skills",
    },
    AgentAdapter {
        id: "kiro",
        label: "Kiro",
        executables: &["kiro-cli"],
        home_markers: &[".kiro"],
        skill_dir: SkillDir::Kiro,
        skill_os: UNIX_OS,
        hook: HookSupport::Guidance(GuidanceAgent::Kiro),
        min_version: Some("3.0.0"),
        default_on: false,
        doc_url: "https://kiro.dev/docs/skills/",
    },
    AgentAdapter {
        id: "qwen-code",
        label: "Qwen Code",
        executables: &["qwen"],
        home_markers: &[".qwen"],
        skill_dir: SkillDir::Qwen,
        skill_os: UNIX_OS,
        hook: HookSupport::Guidance(GuidanceAgent::Qwen),
        min_version: None,
        default_on: false,
        doc_url: "https://qwenlm.github.io/qwen-code-docs/en/users/features/skills/",
    },
    AgentAdapter {
        id: "goose",
        label: "Goose",
        executables: &["goose"],
        home_markers: &[".config/goose"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://goose-docs.ai/docs/guides/context-engineering/using-skills/",
    },
    AgentAdapter {
        id: "cline",
        label: "Cline",
        executables: &["cline"],
        home_markers: &[".cline"],
        skill_dir: SkillDir::Cline,
        skill_os: ALL_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://docs.cline.bot/customization/skills",
    },
    AgentAdapter {
        id: "kilo-code",
        label: "Kilo Code",
        executables: &["kilo"],
        home_markers: &[".config/kilo"],
        skill_dir: SkillDir::Shared,
        skill_os: ALL_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://github.com/Kilo-Org/kilocode/blob/main/packages/kilo-docs/pages/customize/skills.md",
    },
    AgentAdapter {
        id: "crush",
        label: "Crush",
        executables: &["crush"],
        home_markers: &[".config/crush"],
        skill_dir: SkillDir::Shared,
        skill_os: ALL_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://github.com/charmbracelet/crush/blob/main/README.md",
    },
    AgentAdapter {
        id: "junie",
        label: "Junie",
        executables: &["junie"],
        home_markers: &[".junie"],
        skill_dir: SkillDir::Shared,
        skill_os: ALL_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://junie.jetbrains.com/docs/agent-skills.html",
    },
    AgentAdapter {
        id: "augment",
        label: "Augment",
        executables: &["auggie"],
        home_markers: &[".augment"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://docs.augmentcode.com/cli/skills",
    },
    AgentAdapter {
        id: "pi",
        label: "Pi",
        executables: &[],
        home_markers: &[".pi/agent"],
        skill_dir: SkillDir::Shared,
        skill_os: ALL_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/skills.md",
    },
    AgentAdapter {
        id: "grok",
        label: "Grok",
        executables: &[],
        home_markers: &[".grok"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://docs.x.ai/build/features/skills-plugins-marketplaces",
    },
    AgentAdapter {
        id: "kimi-code",
        label: "Kimi Code",
        executables: &[],
        home_markers: &[".kimi-code"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://github.com/MoonshotAI/kimi-code/blob/main/docs/en/customization/skills.md",
    },
    AgentAdapter {
        id: "mistral-vibe",
        label: "Mistral Vibe",
        executables: &[],
        home_markers: &[".vibe"],
        skill_dir: SkillDir::Shared,
        skill_os: UNIX_OS,
        hook: HookSupport::None,
        min_version: None,
        default_on: false,
        doc_url: "https://github.com/mistralai/mistral-vibe/blob/main/README.md",
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

/// The agent whose hook is the kit part `part`, when there is one.
pub(crate) fn adapter_of_part(part: ComponentId) -> Option<&'static AgentAdapter> {
    ADAPTERS
        .iter()
        .find(|adapter| adapter.hook == HookSupport::Part(part))
}

impl AgentAdapter {
    /// Whether the agent's documentation confirms its skill folder here.
    pub fn skill_supported(&self) -> bool {
        self.skill_os.contains(&Os::CURRENT)
    }

    /// The agent's program, when it is on this machine.
    pub fn executable(&self, home: &Path) -> Option<PathBuf> {
        // A test machine's own agents must not decide what a fixture home
        // finds, so under test only the fixture's `~/.local/bin` is searched.
        #[cfg(test)]
        {
            let folder = home.join(".local/bin").into_os_string();
            self.executables
                .iter()
                .find_map(|name| hide_platform::host::find_program(&folder, name))
        }
        #[cfg(not(test))]
        {
            self.executables
                .iter()
                .find_map(|name| hide_agent_hooks::find_binary(name, home))
        }
    }

    /// Whether the agent is set up on this machine: its program is found, or
    /// a folder it creates is there.
    pub fn detected(&self, home: &Path) -> bool {
        self.executable(home).is_some()
            || self
                .home_markers
                .iter()
                .any(|marker| home.join(marker).is_dir())
    }
}

// --- The skill stub ------------------------------------------------------------

/// The text of the stub. It points at the binary and carries no usage, so it
/// stays right as `hide browser help` changes.
fn skill_text() -> String {
    format!(
        "---\nname: {SKILL_NAME}\ndescription: Read and drive a browser display inside Hide. Use when a task involves a web page, a local app in a browser tab, or checking what a page shows.\n---\n\n<!-- {SKILL_MARKER_NAME}@{SKILL_VERSION}: written by Hide; remove it from Settings, Agents -->\n\nRun `hide browser help` and follow what it prints. It explains how to open a page, read it by refs, check a change with `--diff`, and what to do when a command fails.\n"
    )
}

fn skill_marker_version(text: &str) -> Option<u32> {
    let start = text.find(&format!("{SKILL_MARKER_NAME}@"))? + SKILL_MARKER_NAME.len() + 1;
    let digits: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// What one skill folder holds, before the kit decides anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SkillObserved {
    Current,
    Stale,
    Missing,
    /// A file there that Hide did not write; it is left as it is.
    Foreign,
    Unreadable(String),
}

pub(crate) fn observe_skill(dir: SkillDir, home: &Path) -> SkillObserved {
    let path = dir.skill_file(home);
    match std::fs::read_to_string(&path) {
        Ok(text) => match skill_marker_version(&text) {
            Some(version) if version == SKILL_VERSION && text == skill_text() => {
                SkillObserved::Current
            }
            Some(_) => SkillObserved::Stale,
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
    match observe_skill(dir, home) {
        SkillObserved::Missing | SkillObserved::Foreign => Ok(false),
        SkillObserved::Unreadable(reason) => Err(reason),
        SkillObserved::Current | SkillObserved::Stale => {
            let path = dir.skill_file(home);
            std::fs::remove_file(&path)
                .map_err(|error| format!("{} could not be removed: {error}", path.display()))?;
            // Only an empty folder goes; anything else in it is the
            // operator's.
            let _ = std::fs::remove_dir(dir.skill_folder(home));
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
    /// Detected, with a documented skill folder: the switch works.
    Available,
    /// Not found on this machine.
    NotInstalled,
    /// Found, but the agent's documentation gives no skill folder for this
    /// system.
    UnsupportedSystem,
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
    pub skill: PieceReport,
    /// `None` for an agent with no hook (skill only).
    pub hook: Option<PieceReport>,
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
                    .is_some_and(|hook| hook.state.needs_attention()))
    }
}
