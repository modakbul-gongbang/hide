//! Optional structured user-turn content, independent of transcript dialects.
//! Existing readers supply no structure. Later readers can construct this
//! only from records that actually contain it, without inferring UI text.

pub const TEXT_LIMIT_BYTES: usize = 8 * 1024;
pub const CHOICE_LIMIT: usize = 8;
pub const CHOICE_LIMIT_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserTurnKind {
    PlanApproval,
    Question,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserTurnContent {
    text: String,
    choices: Vec<String>,
    truncated: bool,
}

impl UserTurnContent {
    /// Borrowed input is bounded before copying. Even a hostile iterator
    /// is advanced at most CHOICE_LIMIT + 1 times.
    pub fn new<'a>(text: &str, choices: impl IntoIterator<Item = &'a str>) -> Self {
        let (text, mut truncated) = bounded(text, TEXT_LIMIT_BYTES);
        let mut kept = Vec::new();
        for choice in choices.into_iter().take(CHOICE_LIMIT + 1) {
            if kept.len() == CHOICE_LIMIT {
                truncated = true;
                break;
            }
            let (choice, cut) = bounded(choice, CHOICE_LIMIT_BYTES);
            truncated |= cut;
            kept.push(choice);
        }
        Self {
            text,
            choices: kept,
            truncated,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn choices(&self) -> &[String] {
        &self.choices
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
}

fn bounded(value: &str, limit: usize) -> (String, bool) {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), end < value.len())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserTurnFact {
    pub kind: UserTurnKind,
    pub content: Option<UserTurnContent>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_content_preserves_small_records_and_marks_utf8_and_choice_cuts() {
        let short = UserTurnContent::new("Continue?", ["Yes", "No"]);
        assert_eq!(short.text(), "Continue?");
        assert_eq!(short.choices(), ["Yes", "No"]);
        assert!(!short.truncated());
        let text = "질".repeat(TEXT_LIMIT_BYTES);
        let choice = "선".repeat(CHOICE_LIMIT_BYTES);
        let long = UserTurnContent::new(&text, std::iter::repeat(choice.as_str()));
        assert_eq!(long.text().len(), 8190);
        assert_eq!(long.choices().len(), 8);
        assert!(long.choices().iter().all(|choice| choice.len() == 255));
        assert!(long.truncated());
    }

    #[test]
    fn an_existing_plan_wait_has_no_invented_question_structure() {
        let mut tracker = super::super::TurnTracker::default();
        tracker.fold(
            0,
            &super::super::TurnMark::Started {
                turn: Some("turn".into()),
                mode: super::super::TurnMode::Plan,
            },
        );
        tracker.fold(
            1,
            &super::super::TurnMark::Plan {
                turn: Some("turn".into()),
            },
        );
        tracker.fold(
            2,
            &super::super::TurnMark::Completed {
                turn: Some("turn".into()),
            },
        );
        assert_eq!(
            tracker.user_turn(),
            Some(UserTurnFact {
                kind: UserTurnKind::PlanApproval,
                content: None,
            })
        );
    }
}
