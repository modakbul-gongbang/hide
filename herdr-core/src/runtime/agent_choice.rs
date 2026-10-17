//! The agent kind and model a start names, the words they become on the
//! agent's command line, and the choice every start surface preselects next
//! (PRD home-device-rail D-18, D-20).
//!
//! Every start (⌘N, New worktree, an issue's Start, 맡기기, the phone) goes
//! through here, so a pick on any of them is the next default on all of
//! them, and a model reaches the CLI the same way wherever it was chosen.
//!
//! The model and effort a pane's agent was started with are kept here too
//! (`ui_state.pane_launches`), for the conversation it was started for, so
//! every restart Hide makes of it (a wake, a fork, a reopen) starts it with
//! the same choice instead of the CLI's configured default.

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

/// The arguments a restart of a `kind` agent adds for the choice it was
/// started with, after its resume arguments; none for the CLI's defaults.
/// A saved choice the agent does not take (a store edited by hand) is
/// refused with the reason rather than dropped, so the restart says so.
pub(crate) fn launch_arguments(
    kind: &str,
    choice: &hide_agent_adapter::LaunchChoice,
) -> Result<Vec<String>, String> {
    if choice.is_default() {
        return Ok(Vec::new());
    }
    hide_agent_adapter::adapter(kind)
        .and_then(|row| row.start)
        .ok_or_else(|| format!("{kind} takes no model or effort"))?
        .choice_arguments(choice)
}

/// Binds each pane's launch record to the session its agent reports, and
/// drops a record whose pane hosts something else: another kind, another
/// session, or no pane at all in the authoritative topology this pass owns.
/// A row of no known kind (a restored server still detecting it) decides
/// nothing. Returns whether a record moved.
pub(crate) fn settle_pane_launches(
    launches: &mut BTreeMap<String, crate::model::PaneLaunch>,
    agents: &[SidebarAgentSnapshot],
    live_pane_ids: Option<&HashSet<String>>,
    scope: crate::agent_state::ReadRecordScope<'_>,
) -> bool {
    if launches.is_empty() {
        return false;
    }
    let count = launches.len();
    if let Some(live_pane_ids) = live_pane_ids {
        launches.retain(|pane_id, _| !scope.owns(pane_id) || live_pane_ids.contains(pane_id));
    }
    let mut changed = launches.len() != count;
    for agent in agents {
        let Some(launch) = launches.get_mut(&agent.pane_id) else {
            continue;
        };
        let other_kind = hide_agent_adapter::adapter(&agent.agent_kind).is_some()
            && hide_agent_adapter::canonical_kind(&agent.agent_kind) != launch.kind;
        let other_session = match (&launch.session, &agent.session_id) {
            (None, Some(session)) if !other_kind => {
                launch.session = Some(session.clone());
                changed = true;
                false
            }
            (Some(bound), Some(session)) => bound != session,
            _ => false,
        };
        if other_kind || other_session {
            launches.remove(&agent.pane_id);
            changed = true;
        }
    }
    changed
}

impl Runtime {
    /// Records the choice Hide started a `kind` agent with in `pane_id` (the
    /// core's id for the pane), replacing what the pane held: a start on the
    /// CLI's defaults leaves none. `session` is the conversation a restart
    /// resumed there, which the record then already describes.
    pub(crate) fn record_pane_launch(
        &mut self,
        pane_id: &str,
        kind: &str,
        choice: &hide_agent_adapter::LaunchChoice,
        session: Option<&str>,
    ) {
        let launches = &mut self.snapshot.ui_state.pane_launches;
        let changed = if choice.is_default() {
            launches.remove(pane_id).is_some()
        } else {
            let launch = crate::model::PaneLaunch {
                kind: hide_agent_adapter::canonical_kind(kind).to_owned(),
                choice: choice.clone(),
                session: session.map(str::to_owned),
            };
            launches.insert(pane_id.to_owned(), launch.clone()) != Some(launch)
        };
        if changed {
            self.persist_ui_state();
        }
    }

    /// The choice `agent` was started with, when its pane's record describes
    /// this agent: the same kind, and the conversation the record was bound
    /// to (or none bound yet). Otherwise the CLI's defaults.
    pub(crate) fn launch_choice_of(
        &self,
        agent: &SidebarAgentSnapshot,
    ) -> hide_agent_adapter::LaunchChoice {
        self.snapshot
            .ui_state
            .pane_launches
            .get(&agent.pane_id)
            .filter(|launch| {
                hide_agent_adapter::canonical_kind(&agent.agent_kind) == launch.kind
                    && (launch.session.is_none() || launch.session == agent.session_id)
            })
            .map(|launch| launch.choice.clone())
            .unwrap_or_default()
    }

    /// Settles the launch records against one pass's agent rows, and saves
    /// them when one moved.
    pub(super) fn apply_pane_launches(
        &mut self,
        agents: &[SidebarAgentSnapshot],
        scope: crate::agent_state::ReadRecordScope<'_>,
        live_pane_ids: Option<&HashSet<String>>,
    ) {
        if settle_pane_launches(
            &mut self.snapshot.ui_state.pane_launches,
            agents,
            live_pane_ids,
            scope,
        ) {
            self.persist_ui_state();
        }
    }

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
