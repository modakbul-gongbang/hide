//! Core-owned chip choices. A whole UI-state save cannot overwrite them.
use super::model::{Cell, Scope, UsageSnapshot, WindowOutcome};
use hide_agent_adapter::usage::{Support, WindowKind};
use serde::{Deserialize, Serialize};
pub const MAX_PINS: usize = 3;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub agent: String,
    pub window: WindowKind,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsagePreferences {
    pub pinned: Vec<Pin>,
    pub hidden: Vec<String>,
}
impl Default for UsagePreferences {
    fn default() -> Self {
        Self {
            pinned: hide_agent_adapter::ADAPTERS
                .iter()
                .filter(|row| matches!(row.usage.window(WindowKind::Weekly), Support::Supported(_)))
                .map(|row| Pin {
                    agent: row.herdr.name.into(),
                    window: WindowKind::Weekly,
                })
                .collect(),
            hidden: Vec::new(),
        }
    }
}
impl UsagePreferences {
    pub fn valid(&self) -> bool {
        self.pinned.len() <= MAX_PINS
            && self.hidden.len() <= hide_agent_adapter::ADAPTERS.len()
            && self.pinned.iter().enumerate().all(|(i, pin)| {
                supported(&pin.agent, pin.window) && !self.pinned[..i].contains(pin)
            })
            && self
                .hidden
                .iter()
                .enumerate()
                .all(|(i, agent)| supports_limits(agent) && !self.hidden[..i].contains(agent))
    }
    pub fn restored(self) -> Self {
        if self.valid() {
            self
        } else {
            crate::diagnostic!(
                serde_json::json!({"component":"usage", "kind":"preferences.invalid"})
            );
            Self::default()
        }
    }
    pub fn pin(
        &mut self,
        pin: Pin,
        enabled: bool,
        usage: &UsageSnapshot,
    ) -> Result<bool, &'static str> {
        if !supported(&pin.agent, pin.window) {
            return Err("usage.unsupported_window");
        }
        if enabled
            && let Some(row) = usage.own_agent(&pin.agent)
            && let Cell::Reading { value, .. } = &row.limits
            && value.windows.iter().any(|w| {
                w.kind == pin.window
                    && w.scope == Scope::All
                    && matches!(w.outcome, WindowOutcome::NotOffered)
            })
        {
            return Err("usage.not_offered");
        }
        if self.pinned.contains(&pin) == enabled {
            return Ok(false);
        }
        if enabled {
            if self.pinned.len() >= MAX_PINS {
                return Err("usage.pin_limit");
            }
            self.pinned.push(pin);
        } else {
            self.pinned.retain(|p| p != &pin);
        }
        Ok(true)
    }
    pub fn hide(&mut self, agent: String, hidden: bool) -> Result<bool, &'static str> {
        if !supports_limits(&agent) {
            return Err("usage.unsupported_agent");
        }
        if self.hidden.contains(&agent) == hidden {
            return Ok(false);
        }
        if hidden {
            self.hidden.push(agent);
        } else {
            self.hidden.retain(|a| a != &agent);
        }
        Ok(true)
    }
}
fn supported(agent: &str, kind: WindowKind) -> bool {
    hide_agent_adapter::adapter(agent).is_some_and(|row| {
        row.herdr.name == agent && matches!(row.usage.window(kind), Support::Supported(_))
    })
}
fn supports_limits(agent: &str) -> bool {
    hide_agent_adapter::adapter(agent)
        .is_some_and(|row| row.herdr.name == agent && row.usage.limit_sources().next().is_some())
}
