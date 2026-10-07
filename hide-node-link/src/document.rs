//! An opened file as the editor holds it, and the revision a save is checked against.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Text,
    Markdown,
    Image,
    Pdf,
    Binary,
}

impl DocumentKind {
    pub fn is_editable(self) -> bool {
        matches!(self, Self::Text | Self::Markdown)
    }
}

/// One opened file. `contents` and `revision` are present exactly when the
/// file is an editable text document the editor holds in memory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub kind: DocumentKind,
    pub language: Option<String>,
    pub contents: Option<String>,
    pub readonly_reason: Option<String>,
    pub revision: Option<String>,
    pub size_bytes: u64,
}

/// The revision a save is checked against: the SHA-256 of the exact bytes.
/// A content revision, unlike a modification time, changes with every
/// different content and never with a touch that changes nothing.
pub fn revision_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(7 + 64);
    text.push_str("sha256:");
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}
