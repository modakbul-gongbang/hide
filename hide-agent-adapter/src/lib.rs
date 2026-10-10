//! Static agent declarations shared by hooks, sessions, installation and UI.
//!
//! A dialect id selects code in its existing owner. This crate performs no
//! I/O, starts no work and stores no runtime state. Lookup borrows the input
//! and static rows: no allocation, lock, normalization buffer or cache.

mod declarations;
pub use declarations::ADAPTERS;
mod web;
pub use web::{HookKind, WebAdapter, web_contract};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentId {
    ClaudeCode,
    Codex,
    Grok,
    OpenCode,
    Pi,
    Omp,
    Cursor,
}

impl AgentId {
    pub const fn adapter(self) -> &'static AgentAdapter {
        &ADAPTERS[self as usize]
    }

    /// Display priority is independent of whether this build reads a session.
    pub const fn title_priority(self) -> TitlePriority {
        match self {
            Self::ClaudeCode | Self::Codex => TitlePriority::GoalFirst,
            Self::Grok | Self::OpenCode | Self::Pi | Self::Omp => TitlePriority::NativeFirst,
            Self::Cursor => TitlePriority::GoalOnly,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TitlePriority {
    GoalFirst,
    NativeFirst,
    GoalOnly,
}

/// The input and output protocol of an agent's own hook: how its payload
/// names the shell call and session, and how a refusal is written. Claude
/// Code's and Codex's six-event hook, a script file of Hide's in the agent's
/// own folder (OpenCode's plugin, Pi's and omp's extension), which calls the
/// same helper, and Grok's and Cursor's own hook files.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HookDialect {
    ClaudeCode,
    Codex,
    OpenCode,
    Grok,
    Cursor,
    Pi,
    Omp,
}

impl HookDialect {
    pub const ALL: [Self; 7] = [
        Self::ClaudeCode,
        Self::Codex,
        Self::OpenCode,
        Self::Grok,
        Self::Cursor,
        Self::Pi,
        Self::Omp,
    ];

    pub const fn adapter(self) -> &'static AgentAdapter {
        match self {
            Self::ClaudeCode => AgentId::ClaudeCode.adapter(),
            Self::Codex => AgentId::Codex.adapter(),
            Self::OpenCode => AgentId::OpenCode.adapter(),
            Self::Grok => AgentId::Grok.adapter(),
            Self::Cursor => AgentId::Cursor.adapter(),
            Self::Pi => AgentId::Pi.adapter(),
            Self::Omp => AgentId::Omp.adapter(),
        }
    }
}

/// Hide's own hook file for an agent of the basic tier
/// (`hide_agent_hooks::guidance`): the events that agent documents, under
/// one marker, beside every other tool's hooks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuidanceDialect {
    Cursor,
    Grok,
}

impl GuidanceDialect {
    /// Whether the file's session-start entry adds Hide's guidance to the
    /// agent's context. Grok discards what a session-start hook prints.
    pub const fn prints_guidance(self) -> bool {
        matches!(self, Self::Cursor)
    }
}

/// A script file Hide owns in the agent's own plugin or extension folder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginDialect {
    OpenCode,
    /// Pi's and omp's extension: one source, two files.
    Pi,
    Omp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookInstall {
    Runtime(HookDialect),
    Guidance(GuidanceDialect),
    Plugin(PluginDialect),
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionFormat {
    Claude,
    Codex,
    Grok,
    Pi,
    Omp,
    Cursor,
    OpenCode,
}

impl SessionFormat {
    pub const fn adapter(self) -> &'static AgentAdapter {
        match self {
            Self::Claude => AgentId::ClaudeCode.adapter(),
            Self::Codex => AgentId::Codex.adapter(),
            Self::Grok => AgentId::Grok.adapter(),
            Self::Pi => AgentId::Pi.adapter(),
            Self::Omp => AgentId::Omp.adapter(),
            Self::Cursor => AgentId::Cursor.adapter(),
            Self::OpenCode => AgentId::OpenCode.adapter(),
        }
    }

    pub const fn reports_turns(self) -> bool {
        matches!(
            self,
            Self::Claude | Self::Codex | Self::Grok | Self::Omp | Self::OpenCode
        )
    }
    /// Whether Herdr keeps reporting the agent `working` while it waits for
    /// the operator's plan approval or answer, and the session records say
    /// what it waits for as the session is now (Grok's `plan_mode.json`, a
    /// question call with no result yet): such a wait outranks the status.
    /// Codex's plan menu is read from how the last turn ended, which says
    /// nothing once Herdr reports the next turn running, so it does not.
    pub const fn waits_while_working(self) -> bool {
        matches!(self, Self::Grok)
    }
    /// Whether the session records prove which background tasks are still
    /// running and when a new process began. Claude Code's do; Codex writes no
    /// record when a session's command ends, and no other format keeps a
    /// process boundary, so their waits are Herdr's alone (PRD
    /// agent-blocked-state B18, D-26).
    pub const fn reports_wake_devices(self) -> bool {
        matches!(self, Self::Claude)
    }
    pub const fn has_session_file(self) -> bool {
        matches!(
            self,
            Self::Claude | Self::Codex | Self::Grok | Self::Pi | Self::Omp | Self::Cursor
        )
    }

    pub const fn is_jsonl(self) -> bool {
        matches!(
            self,
            Self::Claude | Self::Codex | Self::Grok | Self::Pi | Self::Omp
        )
    }

    /// Native file metadata and the default CLI resolver must agree before
    /// a transcript can authorize a read or a lifecycle effect.
    pub const fn requires_native_file_proof(self) -> bool {
        matches!(self, Self::Grok | Self::Pi | Self::Omp | Self::Cursor)
    }

    /// The native reader proves the session's owner and checkout before a
    /// lifecycle effect (wake, fork, resume) uses its id, and Herdr's
    /// reported id alone grants none of them: every native-file reader, and
    /// OpenCode's database reader.
    pub const fn requires_native_proof(self) -> bool {
        self.requires_native_file_proof() || matches!(self, Self::OpenCode)
    }

    /// The reported reference a native proof takes: OpenCode names a session
    /// by its id in OpenCode's database, a file reader by the file's path.
    pub const fn proof_reference_kind(self) -> &'static str {
        match self {
            Self::OpenCode => "id",
            Self::Claude | Self::Codex | Self::Grok | Self::Pi | Self::Omp | Self::Cursor => "path",
        }
    }

    /// The native reader answers a session's modification time and size.
    pub const fn reports_activity(self) -> bool {
        self.has_session_file() || matches!(self, Self::OpenCode)
    }

    /// The native reader feeds the project session catalog and body search.
    pub const fn searchable(self) -> bool {
        self.has_session_file() || matches!(self, Self::OpenCode)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchDialect {
    Claude,
    Codex,
    Grok,
    OpenCode,
    Pi,
    Omp,
    Cursor,
}

impl LaunchDialect {
    pub const fn adapter(self) -> &'static AgentAdapter {
        match self {
            Self::Claude => AgentId::ClaudeCode.adapter(),
            Self::Codex => AgentId::Codex.adapter(),
            Self::Grok => AgentId::Grok.adapter(),
            Self::OpenCode => AgentId::OpenCode.adapter(),
            Self::Pi => AgentId::Pi.adapter(),
            Self::Omp => AgentId::Omp.adapter(),
            Self::Cursor => AgentId::Cursor.adapter(),
        }
    }

    /// How the CLI takes a model and a reasoning effort at launch; `{}` is
    /// replaced by the value. Verified against Claude Code 2.1 and Codex 0.160;
    /// the other agents declare neither, so a start of theirs takes none.
    pub const fn options(self) -> LaunchOptions {
        match self {
            Self::Claude => LaunchOptions {
                model: &["--model", "{}"],
                effort: &["--effort", "{}"],
                efforts: &["low", "medium", "high", "xhigh", "max"],
            },
            Self::Codex => LaunchOptions {
                model: &["-m", "{}"],
                effort: &["-c", "model_reasoning_effort={}"],
                efforts: &["minimal", "low", "medium", "high", "xhigh"],
            },
            Self::Grok | Self::OpenCode | Self::Pi | Self::Omp | Self::Cursor => LaunchOptions {
                model: &[],
                effort: &[],
                efforts: &[],
            },
        }
    }

    /// The first interactive prompt is an argument, held by the native CLI
    /// through startup questions. OpenCode's positional argument is a project.
    pub const fn prompt_flag(self) -> &'static str {
        match self {
            Self::OpenCode => "--prompt",
            _ => "--",
        }
    }

    /// Verified launch dialects that accept repeated extra permission roots.
    /// A checkout cwd is independent of this optional CLI argument.
    pub const fn accepts_extra_directories(self) -> bool {
        matches!(self, Self::Claude | Self::Codex | Self::Omp | Self::Cursor)
    }

    pub const fn closes_pane_when_sleeping(self) -> bool {
        !matches!(self, Self::Claude | Self::Codex)
    }

    /// Whether the agent, resumed with its `resume_flag`, tells Herdr which
    /// session it runs. Cursor's `sessionStart` hook is Herdr's only source
    /// for that, and Cursor CLI 2026.10.01 does not run it on `--resume`
    /// (issue 938), so the pane that resumed it has no session in Herdr
    /// until whoever started the resume says which one it resumed.
    pub const fn resume_reports_session(self) -> bool {
        !matches!(self, Self::Cursor)
    }

    /// The native selector prefix, independent of whether this build has
    /// implemented and declared the reader needed to resume that agent.
    pub const fn resume_flag(self) -> &'static str {
        match self {
            Self::Codex => "resume",
            Self::Pi => "--session",
            Self::OpenCode => "-s",
            Self::Claude | Self::Grok | Self::Omp | Self::Cursor => "--resume",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchOptions {
    pub model: &'static [&'static str],
    pub effort: &'static [&'static str],
    pub efforts: &'static [&'static str],
}

/// A model name reaches a command line, so it is held to the characters
/// model ids use; anything else is refused rather than quoted.
pub fn valid_model(model: &str) -> bool {
    // A leading dash would read as an option to the agent's CLI.
    !model.is_empty()
        && !model.starts_with('-')
        && model.len() <= 80
        && model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._:/[]".contains(&byte))
}

impl LaunchOptions {
    /// The launch arguments for a model and an effort, refused with the
    /// reason when either is not something this CLI declares.
    pub fn arguments(
        self,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let mut arguments = Vec::new();
        if let Some(model) = model {
            if self.model.is_empty() {
                return Err("this agent takes no model at launch".to_owned());
            }
            if !valid_model(model) {
                return Err(format!("model `{model}` is not a model name"));
            }
            arguments.extend(self.model.iter().map(|part| part.replace("{}", model)));
        }
        if let Some(effort) = effort {
            if self.efforts.is_empty() {
                return Err("this agent takes no effort at launch".to_owned());
            }
            if !self.efforts.contains(&effort) {
                return Err(format!(
                    "effort `{effort}` is not one of {}",
                    self.efforts.join(", ")
                ));
            }
            arguments.extend(self.effort.iter().map(|part| part.replace("{}", effort)));
        }
        Ok(arguments)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FindDialect {
    ClaudeTranscript,
    CodexTranscript,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageDialect {
    Claude,
    Codex,
}

impl UsageDialect {
    pub const fn adapter(self) -> &'static AgentAdapter {
        match self {
            Self::Claude => AgentId::ClaudeCode.adapter(),
            Self::Codex => AgentId::Codex.adapter(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkillLocation {
    Shared,
    Claude,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Os {
    Macos,
    Linux,
    Windows,
}

impl Os {
    pub const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Macos
    } else if cfg!(windows) {
        Self::Windows
    } else {
        Self::Linux
    };
    pub const ALL: &'static [Self] = &[Self::Macos, Self::Linux, Self::Windows];
    pub const UNIX: &'static [Self] = &[Self::Macos, Self::Linux];
}

#[derive(Clone, Copy, Debug)]
pub struct HerdrIntegration {
    pub name: &'static str,
    pub folder: &'static [&'static str],
}

/// These are facts for later Factory consumers, not switches activating them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability<T> {
    Available(T),
    Unavailable,
    Unconfirmed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectAsk {
    pub tools: &'static [&'static str],
    pub denial: HookDialect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FactoryCapabilities {
    pub direct_ask: Capability<DirectAsk>,
    pub user_turn: Capability<SessionFormat>,
    pub turn_end_and_answer: Capability<SessionFormat>,
    pub startup_guidance: Capability<HookInstall>,
    pub resume: Capability<LaunchDialect>,
    pub next_prompt_letters: Capability<HookDialect>,
}

/// Independent gates; adding prompt intake never grants bell or shell denial.
#[derive(Clone, Copy, Debug)]
pub struct AgentAdapter {
    pub key: AgentId,
    pub id: &'static str,
    pub label: &'static str,
    pub picker_label: &'static str,
    pub sidebar_label: Option<&'static str>,
    pub aliases: &'static [&'static str],
    pub executables: &'static [&'static str],
    pub skill_location: SkillLocation,
    pub skill_os: &'static [Os],
    pub hook: HookInstall,
    pub herdr: HerdrIntegration,
    pub default_on: bool,
    pub prompt_hook: Option<HookDialect>,
    pub spawn_guard: Option<HookDialect>,
    pub subagent_counts: Option<HookDialect>,
    pub memory: Option<HookDialect>,
    pub bell: bool,
    pub session: Option<SessionFormat>,
    pub sleep: Option<LaunchDialect>,
    pub fork: Option<LaunchDialect>,
    pub resume: Option<LaunchDialect>,
    pub conversation: Option<SessionFormat>,
    pub titles: Option<SessionFormat>,
    pub start: Option<LaunchDialect>,
    pub find: Option<FindDialect>,
    pub usage: Option<UsageDialect>,
    pub doc_url: &'static str,
    pub install_url: &'static str,
    pub logo_id: &'static str,
    pub terminal_click: bool,
    pub factory: FactoryCapabilities,
}

/// Herdr spellings, canonical ids and case variants share this one boundary.
pub fn adapter(value: &str) -> Option<&'static AgentAdapter> {
    let value = value.trim();
    ADAPTERS.iter().find(|row| {
        row.id.eq_ignore_ascii_case(value)
            || row
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(value))
    })
}

/// Known identities use Herdr's spelling; an unknown captured kind stays
/// byte-for-byte intact, without granting it any adapter capability.
pub fn canonical_kind(value: &str) -> &str {
    adapter(value).map_or(value, |row| row.herdr.name)
}

pub fn start_kind(value: &str) -> Option<&'static str> {
    let row = adapter(value)?;
    row.start.map(|_| row.herdr.name)
}

/// The Herdr kind of the agent whose hook asks, under its canonical id
/// `runtime`, whether to refuse a direct question tool in a Factory worker
/// pane; `None` for an agent with no such tool Hide can refuse.
pub fn direct_ask_kind(runtime: &str) -> Option<&'static str> {
    let row = ADAPTERS.iter().find(|row| row.id == runtime)?;
    matches!(row.factory.direct_ask, Capability::Available(_)).then_some(row.herdr.name)
}

const fn start_count() -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < ADAPTERS.len() {
        if ADAPTERS[index].start.is_some() {
            count += 1;
        }
        index += 1;
    }
    count
}

const fn start_kinds() -> [&'static str; start_count()] {
    let mut result = [""; start_count()];
    let mut source = 0;
    let mut destination = 0;
    while source < ADAPTERS.len() {
        if ADAPTERS[source].start.is_some() {
            result[destination] = ADAPTERS[source].herdr.name;
            destination += 1;
        }
        source += 1;
    }
    result
}

pub const START_KINDS: [&str; start_count()] = start_kinds();

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    Skill,
    Guidance,
    Letters,
    Bell,
    Memory,
    SpawnGuard,
    HerdrIntegration,
    Sleep,
    Fork,
    Start,
    Titles,
}

impl Feature {
    pub const ALL: [Self; 11] = [
        Self::Skill,
        Self::Guidance,
        Self::Letters,
        Self::Bell,
        Self::Memory,
        Self::SpawnGuard,
        Self::HerdrIntegration,
        Self::Sleep,
        Self::Fork,
        Self::Start,
        Self::Titles,
    ];
}

impl AgentAdapter {
    pub const fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Skill | Feature::HerdrIntegration => true,
            Feature::Guidance => match self.hook {
                HookInstall::Runtime(_) | HookInstall::Plugin(_) => true,
                HookInstall::Guidance(dialect) => dialect.prints_guidance(),
                HookInstall::None => false,
            },
            Feature::Letters => self.prompt_hook.is_some(),
            Feature::Bell => self.bell,
            Feature::Memory => self.memory.is_some(),
            Feature::SpawnGuard => self.spawn_guard.is_some(),
            Feature::Sleep => self.sleep.is_some(),
            Feature::Fork => self.fork.is_some(),
            Feature::Start => self.start.is_some(),
            // A label reader alone is not the session features offered by
            // Settings: titles count once the conversation is read.
            Feature::Titles => self.conversation.is_some(),
        }
    }

    pub const fn basic(&self) -> bool {
        self.prompt_hook.is_none() || self.spawn_guard.is_none()
    }
}
