//! The Factory's GitHub access (D-46): a lost sign-in or a missing
//! permission is one state per Factory, a person's single to-do, and every
//! Task whose GitHub step it stopped shows that it waits. While it lasts the
//! Factory asks GitHub nothing it would be refused; a check that passes,
//! pressed by a person or every few minutes on its own, lets the stopped
//! steps continue where they were.

use serde_json::json;

use super::{Engine, Reply, refuse};
use crate::adapters::Failure;
use crate::judgment;
use crate::model::*;

/// How often a blocked Factory's access is checked again on its own.
const ACCESS_RECHECK_MS: u64 = 5 * MINUTE_MS;

impl Engine {
    /// GitHub refused the Factory's sign-in (`forbidden` false) or a
    /// permission; `task` is the Task whose step it stopped.
    pub(super) fn github_blocked_by(
        &mut self,
        factory: &str,
        task: Option<&str>,
        failure: &Failure,
        forbidden: bool,
    ) {
        let now = self.now();
        let mut fresh = false;
        if let Some(f) = self.factories.get_mut(factory)
            && f.github_block.is_none()
        {
            f.github_block = Some(GithubBlock {
                forbidden,
                scope: failure.missing_scope.clone(),
                stage: judgment::cut(&failure.stage, 100),
                since: now,
            });
            fresh = true;
        }
        if fresh {
            self.save_factory(factory);
            self.access_checked_at.insert(factory.to_owned(), now);
            self.record(
                factory,
                task,
                "github.blocked",
                json!({"forbidden": forbidden, "stage": failure.stage}),
            );
        }
        if let Some(task) = task {
            self.with_task(factory, task, |t| t.permission_wait = true);
        }
    }

    pub(super) fn github_blocked(&self, factory: &str) -> bool {
        self.factories
            .get(factory)
            .is_some_and(|f| f.github_block.is_some())
    }

    /// Checks each Factory's lost sign-in again every few minutes, so a
    /// sign-in fixed elsewhere is noticed without a press. A missing
    /// permission is not: the check only reads, and a read passes while the
    /// write GitHub refused still would be, so clearing it on its own would
    /// raise the same to-do again at the next write. The person's press
    /// lets the refused step try again.
    pub(super) fn access_tick(&mut self) {
        let now = self.now();
        let due: Vec<String> = self
            .factories
            .values()
            .filter(|f| !f.closed && f.github_block.as_ref().is_some_and(|b| !b.forbidden))
            .filter(|f| {
                self.access_checked_at
                    .get(&f.id)
                    .is_none_or(|at| now.saturating_sub(*at) >= ACCESS_RECHECK_MS)
            })
            .map(|f| f.id.clone())
            .collect();
        for factory in due {
            let _ = self.check_access(&factory);
        }
    }

    /// The to-do's button: asks GitHub again now (B33).
    pub(super) fn resolve_access(&mut self, factory: &str) -> Reply {
        if !self.github_blocked(factory) {
            return Ok(json!({"message": "GitHub access is not blocked"}));
        }
        self.check_access(factory).map_err(|failure| {
            refuse(
                "github_still_blocked",
                "Sign in again with the command the to-do shows, then check again",
            )
            .with(json!({"stage": failure.stage}))
        })?;
        Ok(json!({"message": "GitHub access is back; the stopped steps continue"}))
    }

    fn check_access(&mut self, factory: &str) -> Result<(), Failure> {
        let now = self.now();
        self.access_checked_at.insert(factory.to_owned(), now);
        let Some(f) = self.factories.get(factory).cloned() else {
            return Ok(());
        };
        if let Err(failure) = self.ports.source.check_access(&f) {
            self.record(
                factory,
                None,
                "github.still_blocked",
                json!({"stage": failure.stage}),
            );
            return Err(failure);
        }
        if let Some(f) = self.factories.get_mut(factory) {
            f.github_block = None;
            f.outside_read_failures = 0;
        }
        self.save_factory(factory);
        self.github_backoff.remove(factory);
        let waiting: Vec<String> = self
            .tasks_of(factory)
            .filter(|t| t.permission_wait)
            .map(|t| t.id.clone())
            .collect();
        for id in waiting {
            self.with_task(factory, &id, |t| t.permission_wait = false);
        }
        self.record(factory, None, "github.restored", json!({}));
        Ok(())
    }
}
