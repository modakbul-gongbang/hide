//! Revision-bound authority decisions for a card and one repository document.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentAuthority {
    Outdated,
    CardNewer,
    Ask,
}

impl DocumentAuthority {
    pub fn by_dates(operator: Option<u64>, document: Option<u64>) -> Self {
        match (operator, document) {
            (Some(operator), Some(document)) if operator > document => Self::CardNewer,
            (Some(operator), Some(document)) if document > operator => Self::Outdated,
            _ => Self::Ask,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentComparison {
    pub authority: DocumentAuthority,
    pub document: String,
    pub document_text: String,
    pub document_at_unix_ms: Option<u64>,
    pub operator_text: String,
    pub operator_at_unix_ms: Option<u64>,
    pub source_session: Option<String>,
}

pub const COMPARISON_EXCERPT_BYTES: usize = 3 * 1024;
pub const PROMOTION_BATCH_LIMIT: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_requires_two_distinct_known_dates() {
        assert_eq!(
            DocumentAuthority::by_dates(Some(20), Some(10)),
            DocumentAuthority::CardNewer
        );
        assert_eq!(
            DocumentAuthority::by_dates(Some(10), Some(20)),
            DocumentAuthority::Outdated
        );
        for dates in [
            (None, Some(10)),
            (Some(10), None),
            (None, None),
            (Some(10), Some(10)),
        ] {
            assert_eq!(
                DocumentAuthority::by_dates(dates.0, dates.1),
                DocumentAuthority::Ask
            );
        }
    }
}
