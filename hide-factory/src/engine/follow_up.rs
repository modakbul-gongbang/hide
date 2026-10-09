//! Follow-up candidates and the person's one-button to-dos.
//!
//! A worker's unrelated finding waits in its Task's follow-up list (D-05,
//! D-31) until a person makes it an issue without the factory label, puts
//! it into the Factory as a labelled issue that starts at once, or
//! discards it; a failed issue leaves its reason on the line and can be
//! pressed again (B19).

use serde_json::json;

use super::{Engine, Reply, TEXT_LIMIT, TITLE_LIMIT, refuse};
use crate::command::FollowUpChoice;
use crate::judgment;
use crate::model::*;
use crate::role::Role;

impl Engine {
    pub(super) fn follow_up(
        &mut self,
        role: &Role,
        task: &str,
        discovery: &str,
        choice: FollowUpChoice,
    ) -> Reply {
        let (factory_id, id) = self.resolve(role, task)?;
        let task = self
            .task(&factory_id, &id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
        let Some((found, follow_up)) = task
            .discoveries
            .iter()
            .find(|d| d.id == discovery)
            .and_then(|d| d.follow_up.clone().map(|f| (d.clone(), f)))
        else {
            return Err(refuse(
                "follow_up_not_found",
                "Name a follow-up candidate of this Task, as hide factory show lists it",
            ));
        };
        if follow_up.state != FollowUpState::Open {
            return Err(refuse(
                "follow_up_settled",
                "This follow-up candidate was already settled",
            )
            .with(json!({"state": follow_up.state.as_str()})));
        }
        let factory = self
            .factories
            .get(&factory_id)
            .cloned()
            .ok_or_else(|| refuse("factory_not_found", "Check hide factory status"))?;
        let now = self.now();
        if choice == FollowUpChoice::Discard {
            self.settle_follow_up(
                &factory_id,
                &id,
                discovery,
                FollowUpState::Discarded,
                None,
                None,
            );
            return Ok(self.task_answer(&factory_id, &id, "follow-up discarded"));
        }
        if factory.source == SourceKind::Github && factory.github_block.is_some() {
            return Err(refuse(
                "github_blocked",
                "Sign in to GitHub again and check access before making an issue",
            ));
        }
        let title = follow_up_title(&found.text);
        let marker = format!("<!-- hide-factory-followup: {factory_id}/{id}/{discovery} -->");
        let source = match (&task.issue, &task.pr) {
            (_, Some(pr)) => format!("{} {}", task.display_id(), pr.url),
            (Some(issue), None) => format!("{} {}", task.display_id(), issue.display()),
            (None, None) => task.display_id(),
        };
        let body = format!(
            "{}\n\n↳ {source}\n\n{marker}",
            judgment::cut(&found.text, TEXT_LIMIT)
        );
        let labelled = choice == FollowUpChoice::Factory;
        // A local Factory's Task is its issue's start; only GitHub makes an
        // issue first.
        if labelled && factory.source == SourceKind::Local {
            let card = Card {
                title: title.clone(),
                goal: judgment::cut(&found.text, TEXT_LIMIT),
                ..Card::default()
            };
            let new_id = self.new_task(&factory_id, card, None, None);
            self.request_review(&factory_id, &new_id, now);
            self.settle_follow_up(
                &factory_id,
                &id,
                discovery,
                FollowUpState::Factory,
                None,
                Some(new_id),
            );
            return Ok(self.task_answer(&factory_id, &id, "follow-up put into the Factory"));
        }
        match self
            .ports
            .source
            .create_follow_up(&factory, &title, &body, &marker, labelled)
        {
            Ok(issue) => {
                let (state, new_task) = if labelled {
                    // The label is the start (B18): the Task exists before
                    // the outside read sees the label, so it is made once. A
                    // press after one that made the issue but failed may find
                    // the read already made it from the label.
                    let new_id = match self.task_for_issue(&factory_id, &issue) {
                        Some(task) => task.id,
                        None => self.labeled_task(&factory_id, issue.clone(), &title, &body),
                    };
                    (FollowUpState::Factory, Some(new_id))
                } else {
                    (FollowUpState::Issue, None)
                };
                self.settle_follow_up(&factory_id, &id, discovery, state, Some(issue), new_task);
                Ok(self.task_answer(&factory_id, &id, "follow-up issue made"))
            }
            Err(failure) => {
                let reason = judgment::cut(&failure.detail, 300);
                self.with_task(&factory_id, &id, |t| {
                    if let Some(f) = t
                        .discoveries
                        .iter_mut()
                        .find(|d| d.id == discovery)
                        .and_then(|d| d.follow_up.as_mut())
                    {
                        f.failure = Some(reason.clone());
                    }
                });
                self.external_failure(&factory_id, Some(&id), &failure);
                Err(
                    refuse("follow_up_failed", "Press it again once the cause is fixed")
                        .with(json!({"stage": failure.stage})),
                )
            }
        }
    }

    fn settle_follow_up(
        &mut self,
        factory: &str,
        id: &str,
        discovery: &str,
        state: FollowUpState,
        issue: Option<IssueRef>,
        task: Option<String>,
    ) {
        let now = self.now();
        let display = issue.as_ref().map(IssueRef::display);
        self.with_task(factory, id, |t| {
            if let Some(f) = t
                .discoveries
                .iter_mut()
                .find(|d| d.id == discovery)
                .and_then(|d| d.follow_up.as_mut())
            {
                f.state = state;
                f.issue = issue;
                f.task = task;
                f.failure = None;
                f.at = now;
            }
            t.log(
                now,
                ActivityEvent::FollowUp {
                    discovery: discovery.to_owned(),
                    state,
                    issue: display,
                },
            );
        });
        self.record(
            factory,
            Some(id),
            "follow_up.settled",
            json!({"discovery": discovery, "state": state.as_str()}),
        );
    }
}

/// A follow-up issue's title: the finding's first line.
fn follow_up_title(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Follow-up");
    judgment::cut(line, TITLE_LIMIT)
}
