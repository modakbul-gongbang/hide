//! Opening a file as an editor document: the kind decision, the editable
//! size, the read-only reason and the content revision a later save is
//! checked against.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ErrorCode, HostError, HostResult};

/// The largest file the editor reads into a document. Past it the shell
/// offers the viewer or a download instead (PRD S3 D-12, S5.5 B8).
pub const MAX_EDITABLE_BYTES: u64 = 16 * 1024 * 1024;

/// The first bytes of every PDF, whatever the file is called.
const PDF_SIGNATURE: &[u8] = b"%PDF-";

/// The image kinds the shells decode with their own image loader. The host
/// does not decode images, so the extension is the decision.
const IMAGE_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "tiff", "heic", "avif"];

pub const PREVIEW_ONLY_REASON: &str = "Files larger than 16 MB are preview-only";
pub const READ_ONLY_REASON: &str = "The file is read-only on disk; editing is disabled";

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

/// Opens `relative` under `dir` as a document; its name decides the language.
pub fn open(dir: &Dir, relative: &Path) -> HostResult<Document> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        // A FIFO must not stall the reader waiting for a writer.
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
    }
    let mut file = dir
        .open_with(relative, &options)
        .map_err(|error| HostError::io(&error, "The selected file could not be read"))?
        .into_std();
    let metadata = file
        .metadata()
        .map_err(|error| HostError::io(&error, "The selected file could not be read"))?;
    if !metadata.is_file() {
        return Err(HostError::new(
            ErrorCode::NotAFile,
            "Only existing regular files can be opened",
        ));
    }
    let language = language_for(relative);
    let document = |kind, contents: Option<String>, readonly_reason: Option<&str>| Document {
        kind,
        language: language.clone(),
        revision: contents.as_deref().map(|text| revision_of(text.as_bytes())),
        contents,
        readonly_reason: readonly_reason.map(str::to_owned),
        size_bytes: metadata.len(),
    };

    // An image or a PDF is drawn from its bytes by the shell, so its size is
    // not the editor's concern and its bytes are never carried here.
    if has_image_extension(relative) {
        return Ok(document(DocumentKind::Image, None, None));
    }
    if starts_with_pdf_signature(&mut file)? {
        return Ok(document(DocumentKind::Pdf, None, None));
    }
    if metadata.len() > MAX_EDITABLE_BYTES {
        return Ok(document(
            DocumentKind::Text,
            None,
            Some(PREVIEW_ONLY_REASON),
        ));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| HostError::io(&error, "The selected file contents could not be read"))?;
    let bytes = read_bounded(&mut file)?;
    let Some(bytes) = bytes else {
        return Ok(document(
            DocumentKind::Text,
            None,
            Some(PREVIEW_ONLY_REASON),
        ));
    };
    let Ok(contents) = String::from_utf8(bytes) else {
        return Ok(document(DocumentKind::Binary, None, None));
    };
    let kind = if language.as_deref() == Some("markdown") {
        DocumentKind::Markdown
    } else {
        DocumentKind::Text
    };
    let reason = metadata
        .permissions()
        .readonly()
        .then_some(READ_ONLY_REASON);
    Ok(document(kind, Some(contents), reason))
}

/// Reads at most the editable size; `None` when the file is larger.
pub(crate) fn read_bounded(file: &mut std::fs::File) -> HostResult<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    file.take(MAX_EDITABLE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| HostError::io(&error, "The selected file contents could not be read"))?;
    Ok((bytes.len() as u64 <= MAX_EDITABLE_BYTES).then_some(bytes))
}

fn has_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            IMAGE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

/// Reads only the signature's worth of bytes: a PDF is recognised by its
/// content so that a file with no extension opens as one, and a large PDF is
/// not read whole to find that out.
fn starts_with_pdf_signature(file: &mut std::fs::File) -> HostResult<bool> {
    let mut header = [0u8; PDF_SIGNATURE.len()];
    let mut filled = 0;
    while filled < header.len() {
        match file.read(&mut header[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(HostError::io(
                    &error,
                    "The selected file contents could not be read",
                ));
            }
        }
    }
    Ok(&header[..filled] == PDF_SIGNATURE)
}

pub fn language_for(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let whole_name = match name.to_ascii_lowercase().as_str() {
        ".gitignore" | ".gitattributes" | ".dockerignore" | ".npmignore" => Some("bash"),
        ".env" | ".editorconfig" => Some("ini"),
        "makefile" | "gnumakefile" => Some("makefile"),
        "dockerfile" => Some("dockerfile"),
        "gemfile" | "rakefile" => Some("ruby"),
        "cmakelists.txt" => Some("cmake"),
        _ => None,
    };
    if let Some(language) = whole_name {
        return Some(language.to_owned());
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())?;
    let language = match extension.as_str() {
        "rs" => "rust",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "json" | "jsonc" | "jsonl" => "json",
        "md" | "markdown" | "mdown" => "markdown",
        "sh" | "bash" | "zsh" | "fish" => "bash",
        "toml" | "ini" | "cfg" => "ini",
        "py" | "pyw" => "python",
        "htm" => "html",
        "scss" | "sass" | "less" => "css",
        _ => extension.as_str(),
    };
    Some(language.to_owned())
}
