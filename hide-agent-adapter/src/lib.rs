//! Static agent declarations shared by hooks, sessions, installation and UI.
//!
//! A dialect id selects code in its existing owner. This crate performs no
//! I/O, starts no work and stores no runtime state. Lookup borrows the input
//! and static rows: no allocation, lock, normalization buffer or cache.

mod declarations;
pub use declarations::ADAPTERS;
mod web;
pub use web::{WebAdapter, web_contract};

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookDialect {
    ClaudeCode,
    Codex,
}

impl HookDialect {
    pub const fn adapter(self) -> &'static AgentAdapter {
        match self {
            Self::ClaudeCode => AgentId::ClaudeCode.adapter(),
            Self::Codex => AgentId::Codex.adapter(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuidanceDialect {
    Cursor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookInstall {
    Runtime(HookDialect),
    Guidance(GuidanceDialect),
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionFormat {
    Claude,
    Codex,
    OpenCode,
}

impl SessionFormat {
    pub const fn adapter(self) -> &'static AgentAdapter {
        match self {
            Self::Claude => AgentId::ClaudeCode.adapter(),
            Self::Codex => AgentId::Codex.adapter(),
            Self::OpenCode => AgentId::OpenCode.adapter(),
        }
    }

    pub const fn reports_turns(self) -> bool {
        matches!(self, Self::Claude | Self::Codex)
    }
    pub const fn has_session_file(self) -> bool {
        matches!(self, Self::Claude | Self::Codex)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchDialect {
    Claude,
    Codex,
}

impl LaunchDialect {
    pub const fn adapter(self) -> &'static AgentAdapter {
        match self {
            Self::Claude => AgentId::ClaudeCode.adapter(),
            Self::Codex => AgentId::Codex.adapter(),
        }
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

/// Artwork shipped by the shell, independent of labels and launch support.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarMark {
    Claude,
    Codex,
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
    pub sidebar_mark: Option<SidebarMark>,
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
    Subagents,
    SpawnGuard,
    HerdrIntegration,
    Sleep,
    Fork,
    Start,
    Titles,
}

impl Feature {
    pub const ALL: [Self; 12] = [
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
    pub const fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Skill | Feature::HerdrIntegration => true,
            Feature::Guidance => !matches!(self.hook, HookInstall::None),
            Feature::Letters => self.prompt_hook.is_some(),
            Feature::Bell => self.bell,
            Feature::Memory => self.memory.is_some(),
            Feature::Subagents => self.subagent_counts.is_some(),
            Feature::SpawnGuard => self.spawn_guard.is_some(),
            Feature::Sleep => self.sleep.is_some(),
            Feature::Fork => self.fork.is_some(),
            Feature::Start => self.start.is_some(),
            // OpenCode's existing label reader is not the session features
            // offered by Settings; keep that established distinction.
            Feature::Titles => self.conversation.is_some() && self.titles.is_some(),
        }
    }

    pub const fn basic(&self) -> bool {
        self.prompt_hook.is_none() || self.spawn_guard.is_none()
    }
}
