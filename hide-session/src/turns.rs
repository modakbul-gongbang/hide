//! Whether an agent's last turn left it waiting for the operator, read from
//! the structured records of its session file (PRD codex-plan-approval-hold
//! D-01..D-04).
//!
//! The tracker knows no agent: an agent's line parser turns its own records
//! into [`TurnMark`]s (`native` owns Claude/Codex records), and the
//! tracker folds them into one derived fact, [`Waiting`], that the doorbell
//! and the sidebar read without looking at the agent's kind. The caller keeps
//! the tracker beside its read checkpoint and hands it back on the next read,
//! so an incremental read continues the turn it was in.

mod content;
pub(crate) mod native;
pub use content::{UserTurnContent, UserTurnFact, UserTurnKind};

use serde::{Deserialize, Serialize};

/// What the last turn leaves the operator to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Waiting {
    /// The agent waits for nothing the operator must answer.
    Nothing,
    /// The agent proposed a plan and waits for the operator to approve it.
    PlanApproval,
    /// A native question tool is waiting for the operator's answer.
    Question,
}

/// The mode a turn's start record names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnMode {
    Plan,
    /// A mode the parser knows runs no plan.
    Other,
    /// No mode, or one the parser does not know: a plan this turn proposes
    /// is not known to wait or not.
    Unknown,
}

/// One structured record about a turn, as an agent's parser reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnMark {
    /// A turn started in `mode`.
    Started {
        turn: Option<String>,
        mode: TurnMode,
    },
    /// The turn proposed a plan.
    Plan { turn: Option<String> },
    /// The provider's structured plan item or explicitly tagged plan body.
    PlanContent {
        turn: Option<String>,
        content: UserTurnContent,
    },
    /// Native question calls and their correlated results in one record.
    Tools(Vec<ToolTurnMark>),
    /// The turn finished.
    Completed { turn: Option<String> },
    /// The turn was interrupted.
    Aborted { turn: Option<String> },
    /// A person's message.
    Human,
    /// A person's message in a format whose native question calls are
    /// known but which supplies no separate task-start record (Claude).
    HumanTurn,
    /// A native interruption, without a turn identifier.
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolTurnMark {
    Asked {
        call: String,
        content: Option<UserTurnContent>,
    },
    Answered {
        call: String,
    },
}

/// A provider can ask several questions in a turn, but no unbounded call
/// history is retained. A ninth distinct native question fails the read.
pub const QUESTION_CALL_LIMIT: usize = 8;
pub const NATIVE_ID_LIMIT_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Plan,
    Other,
    /// The turn's start was not read (it began before where the read
    /// started), so its mode is not known.
    Unseen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum End {
    Completed,
    Aborted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Turn {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "content::deserialize_optional_id"
    )]
    id: Option<String>,
    mode: Mode,
    #[serde(default)]
    plan: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plan_content: Option<UserTurnContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<End>,
    /// A person wrote after the turn ended.
    #[serde(default)]
    answered: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct QuestionCall {
    #[serde(deserialize_with = "content::deserialize_id")]
    call: String,
    content: Option<UserTurnContent>,
    answered: bool,
}

/// The last turn of a session as far as it has been read. Byte offsets
/// deduplicate reread records; native call IDs deduplicate only within the
/// current turn, whose bounded history resets on a new human/start record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnTracker {
    /// The offset of the first record not folded yet. A read that starts
    /// earlier (a reader resuming at its anchor) folds nothing twice.
    through: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last: Option<Turn>,
    /// A person wrote while no turn record had been read, so the records
    /// read are not the ones this tracker knows (a format it does not
    /// read): what the session waits for is not known.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    unstructured: bool,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_calls"
    )]
    questions: Vec<QuestionCall>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    question_capacity: bool,
}

fn deserialize_calls<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<QuestionCall>, D::Error> {
    struct Calls;
    impl<'de> serde::de::Visitor<'de> for Calls {
        type Value = Vec<QuestionCall>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(formatter, "at most {QUESTION_CALL_LIMIT} question calls")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut sequence: A,
        ) -> Result<Self::Value, A::Error> {
            let mut calls = Vec::new();
            while calls.len() < QUESTION_CALL_LIMIT {
                let Some(call) = sequence.next_element()? else {
                    return Ok(calls);
                };
                calls.push(call);
            }
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::custom("user_turn_capacity"));
            }
            Ok(calls)
        }
    }
    deserializer.deserialize_seq(Calls)
}

impl TurnTracker {
    /// Keep the native wait while omitting content an older peer cannot read.
    pub fn clear_user_turn_content(&mut self) {
        if let Some(last) = &mut self.last {
            last.plan_content = None;
        }
        for question in &mut self.questions {
            question.content = None;
        }
    }

    /// Existing records establish the kind of wait, never invented text.
    pub fn user_turn(&self) -> Option<UserTurnFact> {
        match self.waiting()? {
            Waiting::Nothing => None,
            Waiting::PlanApproval => Some(UserTurnFact {
                kind: UserTurnKind::PlanApproval,
                content: self.last.as_ref()?.plan_content.clone(),
            }),
            Waiting::Question => {
                let pending = || self.questions.iter().filter(|question| !question.answered);
                let content = pending()
                    .all(|question| question.content.is_some())
                    .then(|| {
                        UserTurnContent::combine(
                            pending().filter_map(|question| question.content.as_ref()),
                        )
                    });
                Some(UserTurnFact {
                    kind: UserTurnKind::Question,
                    content,
                })
            }
        }
    }

    pub fn capacity_exceeded(&self) -> bool {
        self.question_capacity
    }

    /// Folds the mark of the record at `offset`; a record before what was
    /// already folded is ignored.
    pub fn fold(&mut self, offset: u64, mark: &TurnMark) {
        if offset < self.through {
            return;
        }
        self.through = offset + 1;
        match mark {
            TurnMark::Started { turn, mode } => {
                // A provider may repeat a native start at another byte
                // offset. It must not reopen an answered plan or question.
                if turn.is_some()
                    && self
                        .last
                        .as_ref()
                        .is_some_and(|last| last.id == *turn && last.mode != Mode::Unseen)
                {
                    return;
                }
                self.questions.clear();
                self.question_capacity = false;
                self.last = Some(Turn {
                    id: turn.clone(),
                    mode: match mode {
                        TurnMode::Plan => Mode::Plan,
                        TurnMode::Other => Mode::Other,
                        TurnMode::Unknown => Mode::Unseen,
                    },
                    plan: false,
                    plan_content: None,
                    end: None,
                    answered: false,
                });
            }
            TurnMark::Plan { turn } => self.current(turn).plan = true,
            TurnMark::PlanContent { turn, content } => {
                let current = self.current(turn);
                current.plan = true;
                current.plan_content = Some(content.clone());
            }
            TurnMark::Tools(marks) => {
                for mark in marks {
                    match mark {
                        ToolTurnMark::Asked { call, content } => {
                            if self.questions.iter().any(|known| known.call == *call) {
                                continue;
                            }
                            if self.questions.len() == QUESTION_CALL_LIMIT
                                || call.len() > NATIVE_ID_LIMIT_BYTES
                            {
                                self.question_capacity = true;
                                self.questions.clear();
                                break;
                            }
                            self.questions.push(QuestionCall {
                                call: call.clone(),
                                content: content.clone(),
                                answered: false,
                            });
                        }
                        ToolTurnMark::Answered { call } => {
                            if let Some(question) =
                                self.questions.iter_mut().find(|known| known.call == *call)
                            {
                                question.answered = true;
                                question.content = None;
                            }
                        }
                    }
                }
            }
            TurnMark::Completed { turn } => {
                let current = self.current(turn);
                if current.end != Some(End::Completed) {
                    current.end = Some(End::Completed);
                    current.answered = false;
                }
            }
            TurnMark::Aborted { turn } => {
                if self
                    .last
                    .as_ref()
                    .is_none_or(|last| turn.is_none() || last.id.is_none() || last.id == *turn)
                {
                    self.clear_questions();
                }
                self.current(turn).end = Some(End::Aborted);
            }
            TurnMark::Human => {
                self.questions.clear();
                self.question_capacity = false;
                match self.last.as_mut() {
                    Some(turn) if turn.end.is_some() => turn.answered = true,
                    Some(_) => {}
                    None => self.unstructured = true,
                }
            }
            TurnMark::HumanTurn => {
                self.questions.clear();
                self.question_capacity = false;
                self.unstructured = false;
                self.last = Some(Turn {
                    id: None,
                    mode: Mode::Other,
                    plan: false,
                    plan_content: None,
                    end: None,
                    answered: false,
                });
            }
            TurnMark::Interrupted => {
                self.clear_questions();
                if let Some(turn) = &mut self.last {
                    turn.end = Some(End::Aborted);
                }
            }
        }
    }

    fn clear_questions(&mut self) {
        for question in &mut self.questions {
            question.answered = true;
            question.content = None;
        }
    }

    /// What the last turn leaves the operator to do; `None` when the records
    /// read do not settle it (a running turn whose mode was not plain, a
    /// finished plan whose turn's mode was not read or not known, or a
    /// person's messages with no turn record at all).
    pub fn waiting(&self) -> Option<Waiting> {
        if self.question_capacity {
            return None;
        }
        if self.questions.iter().any(|question| !question.answered) {
            return Some(Waiting::Question);
        }
        let Some(turn) = &self.last else {
            return (!self.unstructured).then_some(Waiting::Nothing);
        };
        // A turn that proposed no plan cannot end on the plan menu. One that
        // did waits when it ran in plan mode; in any other mode the records
        // are not the ones this rule was written for, so it is not known.
        match (turn.end, turn.mode) {
            (Some(_), _) if turn.answered => Some(Waiting::Nothing),
            (Some(End::Aborted), Mode::Plan | Mode::Other) => Some(Waiting::Nothing),
            (_, Mode::Other) | (Some(_), Mode::Unseen) if !turn.plan => Some(Waiting::Nothing),
            (Some(End::Completed), Mode::Plan) => Some(if turn.plan {
                Waiting::PlanApproval
            } else {
                Waiting::Nothing
            }),
            _ => None,
        }
    }

    /// The turn a mark names: the last one when the ids agree or the mark
    /// names none, otherwise a turn whose start was not read. That turn keeps
    /// a plan the last one left unanswered, so a record naming another turn
    /// never clears a wait it did not start a turn after.
    fn current(&mut self, id: &Option<String>) -> &mut Turn {
        let same = match (&self.last, id) {
            (Some(turn), Some(id)) => turn.id.as_ref().is_none_or(|known| known == id),
            (Some(_), None) => true,
            (None, _) => false,
        };
        if !same {
            let pending = self
                .last
                .as_ref()
                .is_some_and(|turn| turn.plan && !turn.answered);
            self.last = Some(Turn {
                id: id.clone(),
                mode: Mode::Unseen,
                plan: pending,
                plan_content: None,
                end: None,
                answered: false,
            });
        }
        self.last.as_mut().expect("a turn was just made")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> Option<String> {
        Some(value.to_owned())
    }

    fn folded(marks: &[TurnMark]) -> TurnTracker {
        let mut tracker = TurnTracker::default();
        for (offset, mark) in marks.iter().enumerate() {
            tracker.fold(offset as u64 * 10, mark);
        }
        tracker
    }

    #[test]
    fn a_finished_plan_turn_waits_until_a_person_writes_or_a_turn_starts() {
        let plan = [
            TurnMark::Started {
                turn: id("t1"),
                mode: TurnMode::Plan,
            },
            TurnMark::Human,
            TurnMark::Plan { turn: id("t1") },
            TurnMark::Completed { turn: id("t1") },
        ];
        assert_eq!(folded(&plan).waiting(), Some(Waiting::PlanApproval));

        let mut answered = plan.to_vec();
        answered.push(TurnMark::Human);
        assert_eq!(folded(&answered).waiting(), Some(Waiting::Nothing));

        let mut next = plan.to_vec();
        next.push(TurnMark::Started {
            turn: id("t2"),
            mode: TurnMode::Other,
        });
        assert_eq!(folded(&next).waiting(), Some(Waiting::Nothing));
    }

    #[test]
    fn a_plan_whose_turn_start_was_not_read_is_unknown() {
        let mut tracker = TurnTracker::default();
        tracker.fold(0, &TurnMark::Plan { turn: id("t1") });
        tracker.fold(10, &TurnMark::Completed { turn: id("t1") });
        assert_eq!(tracker.waiting(), None);
    }

    #[test]
    fn a_mark_already_folded_is_not_folded_again() {
        let mut tracker = folded(&[
            TurnMark::Started {
                turn: id("t1"),
                mode: TurnMode::Plan,
            },
            TurnMark::Plan { turn: id("t1") },
            TurnMark::Completed { turn: id("t1") },
        ]);
        // A reader resuming at an earlier anchor sees the person's message
        // that opened the turn again; it does not answer the plan.
        tracker.fold(5, &TurnMark::Human);
        assert_eq!(tracker.waiting(), Some(Waiting::PlanApproval));
    }

    #[test]
    fn records_the_tracker_does_not_know_never_read_as_nothing_waiting() {
        // A start whose mode is missing or unknown, then a proposed plan.
        let unknown_mode = folded(&[
            TurnMark::Started {
                turn: id("t1"),
                mode: TurnMode::Unknown,
            },
            TurnMark::Plan { turn: id("t1") },
            TurnMark::Completed { turn: id("t1") },
        ]);
        assert_eq!(unknown_mode.waiting(), None);
        // A plan proposed in a turn whose mode says it runs none.
        let plan_in_other = folded(&[
            TurnMark::Started {
                turn: id("t1"),
                mode: TurnMode::Other,
            },
            TurnMark::Plan { turn: id("t1") },
        ]);
        assert_eq!(plan_in_other.waiting(), None);
        let mut ended = plan_in_other.clone();
        ended.fold(100, &TurnMark::Completed { turn: id("t1") });
        assert_eq!(ended.waiting(), None);
        // A person's messages with no turn record read at all.
        assert_eq!(folded(&[TurnMark::Human]).waiting(), None);
        assert_eq!(folded(&[]).waiting(), Some(Waiting::Nothing));
        // A record naming another turn, with no start read, after a wait.
        for stray in [
            TurnMark::Completed { turn: id("t2") },
            TurnMark::Aborted { turn: id("t2") },
        ] {
            let mut tracker = folded(&[
                TurnMark::Started {
                    turn: id("t1"),
                    mode: TurnMode::Plan,
                },
                TurnMark::Plan { turn: id("t1") },
                TurnMark::Completed { turn: id("t1") },
            ]);
            tracker.fold(100, &stray);
            assert_eq!(tracker.waiting(), None, "{stray:?}");
        }
    }
}
