//! The agent kind and model a start names, the words they become on the
//! agent's command line, and the choice every start surface preselects next
//! (PRD home-device-rail D-18, D-20).
//!
//! Every start (⌘N, New worktree, an issue's Start, 맡기기, the phone) goes
//! through here, so a pick on any of them is the next default on all of
//! them, and a model reaches the CLI the same way wherever it was chosen.

use super::*;

/// The longest model id a start may carry. Catalog ids are short
/// (`gpt-6-astra`, `opus`); anything longer is not one of them.
const MODEL_ID_MAX: usize = 64;

/// Whether `model` can be handed to an agent CLI as one argument. Herdr types
/// the start command into the pane's shell, so an id is held to the
/// characters catalog ids use; the CLI itself judges whether it knows it
/// (D-18: a model the CLI refuses fails on its pane, never silently swapped).
pub(crate) fn valid_model_id(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= MODEL_ID_MAX
        && !model.starts_with('-')
        && model.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
        })
}

/// The model a start names, trimmed, with an empty one meaning the CLI's own
/// default. A kind that starts no agent (`terminal`) takes no model.
pub(crate) fn chosen_model(
    agent_kind: Option<&str>,
    model: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
        return Ok(None);
    };
    if agent_kind.is_none() {
        return Err("A terminal starts no agent, so it takes no model".into());
    }
    if !valid_model_id(model) {
        return Err(format!("{model} is not a model name an agent accepts"));
    }
    Ok(Some(model.to_owned()))
}

/// The agent CLI's own arguments for a start: the model, and each folder a
/// Home agent may write through its links (D-08). Every supported start
/// takes `--model <id>`; extra roots are passed only to a launch dialect
/// that accepts them, independently of the checkout cwd.
pub(crate) fn agent_arguments(
    kind: Option<&str>,
    model: Option<&str>,
    add_dirs: &[String],
) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(model) = model {
        args.push("--model".to_owned());
        args.push(model.to_owned());
    }
    if kind
        .and_then(hide_agent_adapter::adapter)
        .and_then(|row| row.start)
        .is_some_and(|dialect| dialect.accepts_extra_directories())
    {
        for dir in add_dirs {
            args.push("--add-dir".to_owned());
            args.push(dir.clone());
        }
    }
    args
}

impl Runtime {
    /// Remembers the kind and model a start named, so every start surface
    /// preselects them next (D-18, D-20). `terminal` is never remembered, and
    /// a start with no model chose the CLI default, which is remembered too.
    pub(super) fn remember_agent_choice(&mut self, agent_kind: Option<&str>, model: Option<&str>) {
        let Some(kind) = agent_kind.and_then(hide_agent_adapter::start_kind) else {
            return;
        };
        let choice = &mut self.snapshot.ui_state.agent_start;
        let before = choice.clone();
        choice.kind = Some(kind.to_owned());
        match model {
            Some(model) => choice.models.insert(kind.to_owned(), model.to_owned()),
            None => choice.models.remove(kind),
        };
        if *choice != before {
            self.persist_ui_state();
        }
    }
}
