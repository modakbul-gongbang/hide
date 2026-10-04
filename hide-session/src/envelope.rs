//! Who sent a message hcoord delivered into a pane (PRD
//! overview-request-view D-19).
//!
//! Every hcoord delivery starts with one header line,
//! `HCOORD_<KIND> <id> from <name> (<participant>)` for a request or a
//! notice, `HCOORD_WATCH_CHECK <name> (<participant>) cycle <n>` for a watch,
//! and a bare `HCOORD_<KIND> <id>` for what hcoord itself says (an answer, a
//! relay, a delivery problem). The header is read structurally, never
//! guessed from the rest of the text; `contracts/hcoord-envelope.json` holds
//! the examples this reader and hcoord's writer both test against.

/// The most characters of a sender's name a message's facts keep.
const MAX_SENDER_CHARS: usize = 128;

/// The sender a message's hcoord header names, or `None` when the message
/// carries no header. hcoord's own notices name `hcoord`.
pub fn envelope_sender(text: &str) -> Option<String> {
    let first = text.trim_start().lines().next()?.trim();
    let (kind, rest) = first.split_once(' ').unwrap_or((first, ""));
    let suffix = kind.strip_prefix("HCOORD_")?;
    if suffix.is_empty()
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
    {
        return None;
    }
    // The name is the sender's own choice; the facts keep a bounded copy and
    // the core decides how it is shown.
    let named = |from: &str| {
        let name = from.split(" (").next().unwrap_or(from).trim();
        (!name.is_empty()).then(|| name.chars().take(MAX_SENDER_CHARS).collect())
    };
    if kind == "HCOORD_WATCH_CHECK" {
        let watched = rest.rsplit_once(" cycle ").map_or(rest, |(name, _)| name);
        return named(watched).or_else(|| Some("hcoord".to_owned()));
    }
    match rest.split_once(" from ") {
        Some((_, from)) => named(from).or_else(|| Some("hcoord".to_owned())),
        None => Some("hcoord".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_contract_example_reads_its_sender() {
        let contract: serde_json::Value =
            serde_json::from_str(include_str!("../../contracts/hcoord-envelope.json")).unwrap();
        for example in contract["examples"].as_array().unwrap() {
            let line = example["first_line"].as_str().unwrap();
            let body = format!(
                "{line}\nreply: hcoord request reply r_1 --as me --body <answer>\nPlease look"
            );
            assert_eq!(
                envelope_sender(&body).as_deref(),
                example["sender"].as_str(),
                "{line}"
            );
        }
    }

    #[test]
    fn a_message_without_a_header_names_no_sender() {
        for text in [
            "fix the tests",
            "  HCOORD in the middle of a sentence",
            "hcoord_request r_1 from x",
            "",
            "Please read HCOORD_REQUEST r_1 from x",
        ] {
            assert_eq!(envelope_sender(text), None, "{text}");
        }
    }
}
