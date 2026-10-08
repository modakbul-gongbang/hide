//! Optional structured user-turn content, independent of transcript dialects.
//! Readers construct this only from records that actually contain it,
//! without inferring UI text.

use serde::{Deserialize, Deserializer, Serialize, de};

pub const TEXT_LIMIT_BYTES: usize = 8 * 1024;
pub const CHOICE_LIMIT: usize = 8;
pub const CHOICE_LIMIT_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserTurnKind {
    PlanApproval,
    Question,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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

    /// Native questions can carry several texts, while the common contract
    /// carries one. Join their exact texts and option labels within the same
    /// global budget, without first making an unbounded intermediate copy.
    pub(crate) fn questions<'a, C>(questions: impl IntoIterator<Item = (&'a str, C)>) -> Self
    where
        C: IntoIterator<Item = &'a str>,
    {
        let mut content = Self::new("", []);
        let mut text_cut = false;
        for (index, (text, choices)) in questions.into_iter().take(CHOICE_LIMIT + 1).enumerate() {
            if index == CHOICE_LIMIT {
                content.truncated = true;
                break;
            }
            if !text_cut && index > 0 {
                text_cut = content.append_text("\n");
            }
            if !text_cut {
                text_cut = content.append_text(text);
            }
            for choice in choices.into_iter().take(CHOICE_LIMIT + 1) {
                if content.choices.len() == CHOICE_LIMIT {
                    content.truncated = true;
                    break;
                }
                let (choice, cut) = bounded(choice, CHOICE_LIMIT_BYTES);
                content.truncated |= cut;
                content.choices.push(choice);
            }
        }
        content
    }

    pub(crate) fn combine<'a>(contents: impl IntoIterator<Item = &'a Self>) -> Self {
        let mut result = Self::new("", []);
        let mut text_cut = false;
        for (index, content) in contents.into_iter().take(CHOICE_LIMIT + 1).enumerate() {
            if index == CHOICE_LIMIT {
                result.truncated = true;
                break;
            }
            if !text_cut && index > 0 {
                text_cut = result.append_text("\n");
            }
            if !text_cut {
                text_cut = result.append_text(content.text());
            }
            result.truncated |= content.truncated;
            for choice in content.choices() {
                if result.choices.len() == CHOICE_LIMIT {
                    result.truncated = true;
                    break;
                }
                result.choices.push(choice.clone());
            }
        }
        result
    }

    fn append_text(&mut self, text: &str) -> bool {
        let available = TEXT_LIMIT_BYTES - self.text.len();
        let (text, cut) = bounded(text, available);
        self.text.push_str(&text);
        self.truncated |= cut;
        cut
    }
}

/// Reject an unbounded peer/store value before retaining it. The producer
/// truncates native content; transport decoding must preserve that bound.
impl<'de> Deserialize<'de> for UserTurnContent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Fields {
            text: BoundedText<TEXT_LIMIT_BYTES>,
            choices: BoundedChoices,
            truncated: bool,
        }
        let fields = Fields::deserialize(deserializer)?;
        Ok(Self {
            text: fields.text.0,
            choices: fields.choices.0,
            truncated: fields.truncated,
        })
    }
}

struct BoundedText<const LIMIT: usize>(String);

pub(super) fn deserialize_id<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    BoundedText::<{ super::NATIVE_ID_LIMIT_BYTES }>::deserialize(deserializer).map(|value| value.0)
}

pub(super) fn deserialize_optional_id<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<BoundedText<{ super::NATIVE_ID_LIMIT_BYTES }>>::deserialize(deserializer)
        .map(|value| value.map(|value| value.0))
}

impl<'de, const LIMIT: usize> Deserialize<'de> for BoundedText<LIMIT> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TextVisitor<const LIMIT: usize>;
        impl<'de, const LIMIT: usize> de::Visitor<'de> for TextVisitor<LIMIT> {
            type Value = BoundedText<LIMIT>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(formatter, "text of at most {LIMIT} bytes")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.len() > LIMIT {
                    return Err(E::custom("user_turn_content_capacity"));
                }
                Ok(BoundedText(value.to_owned()))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                if value.len() > LIMIT {
                    return Err(E::custom("user_turn_content_capacity"));
                }
                Ok(BoundedText(value))
            }
        }
        deserializer.deserialize_string(TextVisitor::<LIMIT>)
    }
}

struct BoundedChoices(Vec<String>);

impl<'de> Deserialize<'de> for BoundedChoices {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ChoicesVisitor;
        impl<'de> de::Visitor<'de> for ChoicesVisitor {
            type Value = BoundedChoices;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(formatter, "at most {CHOICE_LIMIT} bounded choices")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut choices = Vec::new();
                while choices.len() < CHOICE_LIMIT {
                    let Some(choice) =
                        sequence.next_element::<BoundedText<CHOICE_LIMIT_BYTES>>()?
                    else {
                        return Ok(BoundedChoices(choices));
                    };
                    choices.push(choice.0);
                }
                if sequence.next_element::<de::IgnoredAny>()?.is_some() {
                    return Err(de::Error::custom("user_turn_content_capacity"));
                }
                Ok(BoundedChoices(choices))
            }
        }
        deserializer.deserialize_seq(ChoicesVisitor)
    }
}

fn bounded(value: &str, limit: usize) -> (String, bool) {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), end < value.len())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
