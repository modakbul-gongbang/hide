use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandId {
    PaneZoom,
    NewTab,
    SplitRight,
    SplitDown,
    ClipboardCopy,
    ClipboardPaste,
    NavigatorWorkspaces,
    NavigatorAgents,
    NavigatorWorktrees,
    WorkspaceStatus,
}

impl CommandId {
    pub const ALL: [Self; 10] = [
        Self::PaneZoom,
        Self::NewTab,
        Self::SplitRight,
        Self::SplitDown,
        Self::ClipboardCopy,
        Self::ClipboardPaste,
        Self::NavigatorWorkspaces,
        Self::NavigatorAgents,
        Self::NavigatorWorktrees,
        Self::WorkspaceStatus,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::PaneZoom => "Toggle pane zoom",
            Self::NewTab => "New tab",
            Self::SplitRight => "Split pane right",
            Self::SplitDown => "Split pane down",
            Self::ClipboardCopy => "Copy terminal selection",
            Self::ClipboardPaste => "Paste into terminal",
            Self::NavigatorWorkspaces => "Show workspaces",
            Self::NavigatorAgents => "Show agents",
            Self::NavigatorWorktrees => "Show worktrees",
            Self::WorkspaceStatus => "Show workspace status",
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct ShortcutModifiers(u8);

impl ShortcutModifiers {
    pub const COMMAND: Self = Self(1 << 0);
    pub const SHIFT: Self = Self(1 << 1);
    pub const OPTION: Self = Self(1 << 2);
    pub const CONTROL: Self = Self(1 << 3);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn command_shift() -> Self {
        Self(Self::COMMAND.0 | Self::SHIFT.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl std::ops::BitOr for ShortcutModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Shortcut {
    pub key_code: u16,
    pub modifiers: ShortcutModifiers,
}

impl Shortcut {
    pub const fn new(key_code: u16, modifiers: ShortcutModifiers) -> Self {
        Self {
            key_code,
            modifiers,
        }
    }

    pub fn matches(self, key_code: u16, modifiers: ShortcutModifiers) -> bool {
        self.key_code == key_code && self.modifiers.bits() == modifiers.bits()
    }

    pub fn display(self) -> String {
        let mut value = String::new();
        if self.modifiers.contains(ShortcutModifiers::CONTROL) {
            value.push('^');
        }
        if self.modifiers.contains(ShortcutModifiers::OPTION) {
            value.push('⌥');
        }
        if self.modifiers.contains(ShortcutModifiers::SHIFT) {
            value.push('⇧');
        }
        if self.modifiers.contains(ShortcutModifiers::COMMAND) {
            value.push('⌘');
        }
        value.push_str(key_code_label(self.key_code));
        value
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandScope {
    Window,
    Global,
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegistrationState {
    NotRequested,
    Registered,
    Denied,
    Conflict,
}

pub trait GlobalShortcutRegistrar {
    fn register(&mut self, shortcut: Shortcut, command: CommandId) -> RegistrationState;
}

#[derive(Clone, Debug, Default)]
pub struct DeterministicGlobalRegistrar {
    registrations: BTreeMap<Shortcut, CommandId>,
    denied: BTreeSet<Shortcut>,
}

impl DeterministicGlobalRegistrar {
    pub fn deny(&mut self, shortcut: Shortcut) {
        self.denied.insert(shortcut);
    }

    pub fn registered_command(&self, shortcut: Shortcut) -> Option<CommandId> {
        self.registrations.get(&shortcut).copied()
    }
}

impl GlobalShortcutRegistrar for DeterministicGlobalRegistrar {
    fn register(&mut self, shortcut: Shortcut, command: CommandId) -> RegistrationState {
        if self.denied.contains(&shortcut) {
            return RegistrationState::Denied;
        }
        if let Some(existing) = self.registrations.get(&shortcut) {
            if *existing != command {
                return RegistrationState::Conflict;
            }
        }
        self.registrations.insert(shortcut, command);
        RegistrationState::Registered
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandBinding {
    pub command: CommandId,
    pub scope: CommandScope,
    pub precedence: u8,
    pub default: Shortcut,
    pub current: Shortcut,
    pub registration: RegistrationState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShortcutChangeError {
    UnknownCommand(CommandId),
    Reserved(Shortcut),
    Conflict {
        requested: Shortcut,
        existing: CommandId,
    },
}

impl std::fmt::Display for ShortcutChangeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCommand(command) => write!(formatter, "unknown command {command:?}"),
            Self::Reserved(shortcut) => {
                write!(formatter, "reserved shortcut {}", shortcut.display())
            }
            Self::Conflict {
                requested,
                existing,
            } => write!(
                formatter,
                "shortcut {} is already assigned to {existing:?}",
                requested.display()
            ),
        }
    }
}

impl std::error::Error for ShortcutChangeError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsError {
    Io(String),
    InvalidJson(String),
    UnsupportedSchema(u32),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "settings I/O failed: {message}"),
            Self::InvalidJson(message) => write!(formatter, "settings JSON invalid: {message}"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "settings schema unsupported: {version}")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct PersistedSettings {
    schema_version: u32,
    bindings: BTreeMap<CommandId, Shortcut>,
}

#[derive(Clone, Debug)]
pub struct CommandRegistry {
    bindings: BTreeMap<CommandId, CommandBinding>,
    reserved: BTreeSet<Shortcut>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        let defaults = [
            (
                CommandId::PaneZoom,
                CommandScope::Window,
                100,
                Shortcut::new(36, ShortcutModifiers::command_shift()),
            ),
            (
                CommandId::NewTab,
                CommandScope::Window,
                100,
                Shortcut::new(17, ShortcutModifiers::COMMAND),
            ),
            (
                CommandId::SplitRight,
                CommandScope::Window,
                100,
                Shortcut::new(2, ShortcutModifiers::COMMAND),
            ),
            (
                CommandId::SplitDown,
                CommandScope::Window,
                100,
                Shortcut::new(2, ShortcutModifiers::COMMAND | ShortcutModifiers::SHIFT),
            ),
            (
                CommandId::ClipboardCopy,
                CommandScope::Terminal,
                80,
                Shortcut::new(8, ShortcutModifiers::COMMAND),
            ),
            (
                CommandId::ClipboardPaste,
                CommandScope::Terminal,
                80,
                Shortcut::new(9, ShortcutModifiers::COMMAND),
            ),
            (
                CommandId::NavigatorWorkspaces,
                CommandScope::Window,
                90,
                Shortcut::new(18, ShortcutModifiers::command_shift()),
            ),
            (
                CommandId::NavigatorAgents,
                CommandScope::Window,
                90,
                Shortcut::new(19, ShortcutModifiers::command_shift()),
            ),
            (
                CommandId::NavigatorWorktrees,
                CommandScope::Window,
                90,
                Shortcut::new(20, ShortcutModifiers::command_shift()),
            ),
            (
                CommandId::WorkspaceStatus,
                CommandScope::Global,
                110,
                Shortcut::new(48, ShortcutModifiers::OPTION),
            ),
        ];
        let bindings = defaults
            .into_iter()
            .map(|(command, scope, precedence, shortcut)| {
                (
                    command,
                    CommandBinding {
                        command,
                        scope,
                        precedence,
                        default: shortcut,
                        current: shortcut,
                        registration: RegistrationState::NotRequested,
                    },
                )
            })
            .collect();
        let reserved = [
            Shortcut::new(12, ShortcutModifiers::COMMAND), // Cmd+Q
            Shortcut::new(13, ShortcutModifiers::COMMAND), // Cmd+W
        ]
        .into_iter()
        .collect();
        Self { bindings, reserved }
    }
}

impl CommandRegistry {
    pub fn bindings(&self) -> impl Iterator<Item = &CommandBinding> {
        self.bindings.values()
    }

    pub fn binding(&self, command: CommandId) -> Option<&CommandBinding> {
        self.bindings.get(&command)
    }

    pub fn resolve(&self, key_code: u16, modifiers: ShortcutModifiers) -> Option<CommandId> {
        self.bindings
            .values()
            .filter(|binding| binding.current.matches(key_code, modifiers))
            .max_by_key(|binding| binding.precedence)
            .map(|binding| binding.command)
    }

    pub fn search(&self, query: &str) -> Vec<&CommandBinding> {
        let query = query.trim().to_ascii_lowercase();
        self.bindings
            .values()
            .filter(|binding| {
                query.is_empty()
                    || binding
                        .command
                        .label()
                        .to_ascii_lowercase()
                        .contains(&query)
                    || binding
                        .current
                        .display()
                        .to_ascii_lowercase()
                        .contains(&query)
            })
            .collect()
    }

    pub fn assign(
        &mut self,
        command: CommandId,
        shortcut: Shortcut,
    ) -> Result<(), ShortcutChangeError> {
        if !self.bindings.contains_key(&command) {
            return Err(ShortcutChangeError::UnknownCommand(command));
        }
        if self.reserved.contains(&shortcut) {
            return Err(ShortcutChangeError::Reserved(shortcut));
        }
        if let Some(existing) = self
            .bindings
            .values()
            .find(|binding| binding.command != command && binding.current == shortcut)
        {
            return Err(ShortcutChangeError::Conflict {
                requested: shortcut,
                existing: existing.command,
            });
        }
        let binding = self
            .bindings
            .get_mut(&command)
            .expect("command existence checked above");
        binding.current = shortcut;
        binding.registration = RegistrationState::NotRequested;
        Ok(())
    }

    pub fn set_registration(&mut self, command: CommandId, state: RegistrationState) {
        if let Some(binding) = self.bindings.get_mut(&command) {
            binding.registration = state;
        }
    }

    pub fn register_global<R: GlobalShortcutRegistrar>(
        &mut self,
        command: CommandId,
        registrar: &mut R,
    ) -> Result<RegistrationState, ShortcutChangeError> {
        let binding = self
            .bindings
            .get(&command)
            .ok_or(ShortcutChangeError::UnknownCommand(command))?;
        let state = registrar.register(binding.current, command);
        self.set_registration(command, state);
        Ok(state)
    }

    pub fn reset(&mut self, command: CommandId) -> Result<(), ShortcutChangeError> {
        let default = self
            .bindings
            .get(&command)
            .ok_or(ShortcutChangeError::UnknownCommand(command))?
            .default;
        self.assign(command, default)
    }

    pub fn reset_all(&mut self) {
        let defaults: Vec<_> = self
            .bindings
            .values()
            .map(|binding| (binding.command, binding.default))
            .collect();
        for (command, shortcut) in defaults {
            let _ = self.assign(command, shortcut);
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let persisted = PersistedSettings {
            schema_version: SETTINGS_SCHEMA_VERSION,
            bindings: self
                .bindings
                .iter()
                .map(|(command, binding)| (*command, binding.current))
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&persisted)
            .map_err(|error| SettingsError::InvalidJson(error.to_string()))?;
        atomic_write(path, &bytes)
    }

    pub fn load_or_default(path: &Path) -> Result<Self, SettingsError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(SettingsError::Io(error.to_string())),
        };
        let persisted: PersistedSettings = serde_json::from_slice(&bytes)
            .map_err(|error| SettingsError::InvalidJson(error.to_string()))?;
        if persisted.schema_version != SETTINGS_SCHEMA_VERSION {
            return Err(SettingsError::UnsupportedSchema(persisted.schema_version));
        }
        let mut registry = Self::default();
        for (command, shortcut) in persisted.bindings {
            registry
                .assign(command, shortcut)
                .map_err(|error| SettingsError::InvalidJson(error.to_string()))?;
        }
        Ok(registry)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), SettingsError> {
    let parent = path
        .parent()
        .ok_or_else(|| SettingsError::Io("settings path has no parent".to_owned()))?;
    fs::create_dir_all(parent).map_err(|error| SettingsError::Io(error.to_string()))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, bytes).map_err(|error| SettingsError::Io(error.to_string()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(SettingsError::Io(error.to_string()));
    }
    Ok(())
}

fn key_code_label(key_code: u16) -> &'static str {
    match key_code {
        2 => "D",
        8 => "C",
        9 => "V",
        12 => "Q",
        13 => "W",
        17 => "T",
        18 => "1",
        19 => "2",
        20 => "3",
        36 | 76 => "Enter",
        48 => "Tab",
        _ => "Key",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandId, CommandRegistry, DeterministicGlobalRegistrar, GlobalShortcutRegistrar,
        RegistrationState, Shortcut, ShortcutChangeError, ShortcutModifiers,
    };

    #[test]
    fn default_registry_resolves_zoom_and_option_tab() {
        let registry = CommandRegistry::default();
        assert_eq!(
            registry.resolve(36, ShortcutModifiers::command_shift()),
            Some(CommandId::PaneZoom)
        );
        assert_eq!(
            registry.resolve(48, ShortcutModifiers::OPTION),
            Some(CommandId::WorkspaceStatus)
        );
    }

    #[test]
    fn settings_capture_rejects_reserved_and_duplicate_bindings_without_mutating() {
        let mut registry = CommandRegistry::default();
        let original = registry.binding(CommandId::NewTab).unwrap().current;
        assert_eq!(
            registry.assign(
                CommandId::NewTab,
                Shortcut::new(12, ShortcutModifiers::COMMAND)
            ),
            Err(ShortcutChangeError::Reserved(Shortcut::new(
                12,
                ShortcutModifiers::COMMAND
            )))
        );
        assert_eq!(
            registry.binding(CommandId::NewTab).unwrap().current,
            original
        );
        let duplicate = registry.binding(CommandId::SplitRight).unwrap().current;
        assert!(matches!(
            registry.assign(CommandId::NewTab, duplicate),
            Err(ShortcutChangeError::Conflict { .. })
        ));
        assert_eq!(
            registry.binding(CommandId::NewTab).unwrap().current,
            original
        );
    }

    #[test]
    fn settings_persist_atomically_and_reset_converges() {
        let directory =
            std::env::temp_dir().join(format!("herdr-command-settings-{}", std::process::id()));
        let path = directory.join("shortcuts.json");
        let _ = std::fs::remove_dir_all(&directory);
        let mut registry = CommandRegistry::default();
        let custom = Shortcut::new(0, ShortcutModifiers::COMMAND | ShortcutModifiers::OPTION);
        registry.assign(CommandId::NewTab, custom).unwrap();
        registry.save(&path).unwrap();
        let loaded = CommandRegistry::load_or_default(&path).unwrap();
        assert_eq!(loaded.binding(CommandId::NewTab).unwrap().current, custom);
        let mut loaded = loaded;
        loaded.reset_all();
        loaded.save(&path).unwrap();
        let reset = CommandRegistry::load_or_default(&path).unwrap();
        assert_eq!(
            reset.binding(CommandId::NewTab).unwrap().current,
            reset.binding(CommandId::NewTab).unwrap().default
        );
        assert_eq!(
            reset.binding(CommandId::NewTab).unwrap().registration,
            RegistrationState::NotRequested
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn search_returns_user_visible_command_and_binding() {
        let registry = CommandRegistry::default();
        let matches = registry.search("zoom");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].command, CommandId::PaneZoom);
        assert_eq!(matches[0].current.display(), "⇧⌘Enter");
    }

    #[test]
    fn global_registration_surfaces_denied_and_conflict_states() {
        let shortcut = Shortcut::new(48, ShortcutModifiers::OPTION);
        let mut registry = CommandRegistry::default();
        let mut registrar = DeterministicGlobalRegistrar::default();
        registrar.deny(shortcut);
        assert_eq!(
            registry
                .register_global(CommandId::WorkspaceStatus, &mut registrar)
                .unwrap(),
            RegistrationState::Denied
        );
        let other = Shortcut::new(0, ShortcutModifiers::OPTION);
        registry.assign(CommandId::WorkspaceStatus, other).unwrap();
        assert_eq!(
            registry
                .register_global(CommandId::WorkspaceStatus, &mut registrar)
                .unwrap(),
            RegistrationState::Registered
        );
        assert_eq!(
            registrar.register(other, CommandId::NewTab),
            RegistrationState::Conflict
        );
    }
}
