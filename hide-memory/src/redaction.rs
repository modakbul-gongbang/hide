use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactedText {
    pub text: String,
    pub redactions: usize,
    pub contains_secret_candidate: bool,
}

fn patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r#"(?i)\b(?:api[_-]?key|access[_-]?token|client[_-]?secret|password)\s*[:=]\s*['"]?[^\s'"]{8,}"#,
            r"\bsk-[A-Za-z0-9_-]{16,}\b",
            r"\b(?:ghp|github_pat)_[A-Za-z0-9_]{20,}\b",
            r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----",
            r"\bAKIA[0-9A-Z]{16}\b",
            r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]{16,}\b",
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("static credential pattern"))
        .collect()
    })
}

pub fn redact(input: &str) -> RedactedText {
    let mut text = input.to_owned();
    let mut redactions = 0;
    for pattern in patterns() {
        let count = pattern.find_iter(&text).count();
        if count > 0 {
            redactions += count;
            text = pattern.replace_all(&text, "[REDACTED]").into_owned();
        }
    }
    RedactedText {
        text,
        redactions,
        contains_secret_candidate: redactions > 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_credentials_are_removed_before_any_caller_can_store_them() {
        let source = "Use api_key=super-secret-value-123 and Bearer abcdefghijklmnopqrstuvwxyz";
        let result = redact(source);
        assert_eq!(result.redactions, 2);
        assert!(result.contains_secret_candidate);
        assert!(!result.text.contains("super-secret"));
        assert!(!result.text.contains("abcdefghijkl"));
    }

    #[test]
    fn ordinary_project_text_is_unchanged() {
        let source = "Keep one SQLite writer and fail open in the hook.";
        assert_eq!(
            redact(source),
            RedactedText {
                text: source.to_owned(),
                redactions: 0,
                contains_secret_candidate: false
            }
        );
    }
}
