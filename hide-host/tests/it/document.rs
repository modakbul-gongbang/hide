//! The document decision a shell draws from: kind, editable size, language
//! and the revision a save is checked against (PRD S3 D-01/D-02, S5.5 B8).

use std::fs;
use std::path::Path;

use hide_host::Root;
use hide_host::document::{
    self, DocumentKind, MAX_EDITABLE_BYTES, PREVIEW_ONLY_REASON, language_for, revision_of,
};

fn checkout() -> (tempfile::TempDir, Root) {
    let outer = tempfile::tempdir().unwrap();
    let root = Root::open(outer.path()).unwrap();
    (outer, root)
}

/// A PDF is its signature, not its name, so it opens as one with any
/// extension or none; it carries no bytes and no read-only reason.
#[test]
fn a_pdf_is_recognised_by_its_signature_whatever_it_is_called() {
    let (outer, root) = checkout();
    for name in ["report.pdf", "report", "report.txt"] {
        fs::write(
            outer.path().join(name),
            b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n",
        )
        .unwrap();
        let opened = document::open(root.dir(), Path::new(name)).unwrap();
        assert_eq!(opened.kind, DocumentKind::Pdf, "{name}");
        assert_eq!(opened.contents, None, "{name}");
        assert_eq!(opened.revision, None, "{name}");
        assert_eq!(opened.readonly_reason, None, "{name}");
    }
}

#[test]
fn a_file_named_pdf_without_the_signature_is_not_a_pdf() {
    let (outer, root) = checkout();
    fs::write(outer.path().join("notes.pdf"), b"just text").unwrap();
    fs::write(outer.path().join("blob.pdf"), [0xFF, 0xFE, 0x00, 0x80]).unwrap();
    let text = document::open(root.dir(), Path::new("notes.pdf")).unwrap();
    assert_eq!(text.kind, DocumentKind::Text);
    assert_eq!(text.revision, Some(revision_of(b"just text")));
    let binary = document::open(root.dir(), Path::new("blob.pdf")).unwrap();
    assert_eq!(binary.kind, DocumentKind::Binary);
    assert_eq!(binary.contents, None);
    assert_eq!(binary.readonly_reason, None);
}

#[test]
fn images_markdown_text_and_empty_files_take_their_kinds() {
    let (outer, root) = checkout();
    let cases: [(&str, &[u8], DocumentKind); 6] = [
        ("shot.PNG", &[0x89, b'P', b'N', b'G'], DocumentKind::Image),
        ("photo.heic", b"", DocumentKind::Image),
        ("README.md", b"# hi", DocumentKind::Markdown),
        ("notes.mdown", b"# hi", DocumentKind::Markdown),
        ("main.rs", b"fn main() {}", DocumentKind::Text),
        ("empty", b"", DocumentKind::Text),
    ];
    for (name, bytes, expected) in cases {
        fs::write(outer.path().join(name), bytes).unwrap();
        let opened = document::open(root.dir(), Path::new(name)).unwrap();
        assert_eq!(opened.kind, expected, "{name}");
        assert_eq!(opened.contents.is_some(), expected.is_editable(), "{name}");
    }
}

#[test]
fn viewer_languages_cover_extensionless_configuration_and_json() {
    assert_eq!(
        language_for(Path::new(".gitignore")).as_deref(),
        Some("bash")
    );
    assert_eq!(
        language_for(Path::new("Makefile")).as_deref(),
        Some("makefile")
    );
    assert_eq!(
        language_for(Path::new("settings.jsonc")).as_deref(),
        Some("json")
    );
    assert_eq!(
        language_for(Path::new("manifest.json")).as_deref(),
        Some("json")
    );
    assert_eq!(language_for(Path::new("LICENSE")), None);
}

#[test]
fn a_document_past_the_editable_cap_opens_as_a_preview() {
    let (outer, root) = checkout();
    // Sparse: the file reports the size without holding the bytes.
    fs::File::create(outer.path().join("big.txt"))
        .unwrap()
        .set_len(MAX_EDITABLE_BYTES + 1)
        .unwrap();
    let big = document::open(root.dir(), Path::new("big.txt")).unwrap();
    assert_eq!(big.kind, DocumentKind::Text);
    assert_eq!(big.contents, None);
    assert_eq!(big.readonly_reason.as_deref(), Some(PREVIEW_ONLY_REASON));
    fs::File::create(outer.path().join("at-cap.txt"))
        .unwrap()
        .set_len(MAX_EDITABLE_BYTES)
        .unwrap();
    let at_cap = document::open(root.dir(), Path::new("at-cap.txt")).unwrap();
    assert!(at_cap.contents.is_some(), "the cap itself is editable");
}

#[test]
fn a_folder_and_a_missing_file_are_refused_as_what_they_are() {
    let (outer, root) = checkout();
    fs::create_dir(outer.path().join("src")).unwrap();
    assert_eq!(
        document::open(root.dir(), Path::new("src"))
            .unwrap_err()
            .code,
        hide_host::ErrorCode::NotAFile
    );
    assert_eq!(
        document::open(root.dir(), Path::new("gone.txt"))
            .unwrap_err()
            .code,
        hide_host::ErrorCode::NotFound
    );
}
