//! The save contract (PRD S5.5 B13-B15): a save never discards a change that
//! was complete at the path before it took effect, never leaves the original
//! truncated, and refuses a target it cannot replace faithfully.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use hide_host::document::{self, revision_of};
use hide_host::save::{SaveStage, save, save_observed};
use hide_host::{ErrorCode, Root};

struct Fixture {
    _outer: tempfile::TempDir,
    checkout: PathBuf,
    root: Root,
}

fn fixture(file: &str, contents: &str) -> Fixture {
    let outer = tempfile::tempdir().unwrap();
    let checkout = outer.path().join("checkout");
    fs::create_dir(&checkout).unwrap();
    fs::write(checkout.join(file), contents).unwrap();
    let root = Root::open(&checkout).unwrap();
    Fixture {
        _outer: outer,
        checkout,
        root,
    }
}

fn leftovers(folder: &Path) -> Vec<String> {
    fs::read_dir(folder)
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.contains(".hide-save-"))
        .collect()
}

#[test]
fn a_save_replaces_the_contents_and_keeps_the_permissions() {
    let f = fixture("a.txt", "old");
    fs::set_permissions(f.checkout.join("a.txt"), fs::Permissions::from_mode(0o640)).unwrap();
    let opened = document::open(f.root.dir(), Path::new("a.txt")).unwrap();
    let saved = save(
        f.root.dir(),
        Path::new("a.txt"),
        b"new",
        opened.revision.as_deref().unwrap(),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(f.checkout.join("a.txt")).unwrap(), "new");
    assert_eq!(saved.revision, revision_of(b"new"));
    let mode = fs::metadata(f.checkout.join("a.txt"))
        .unwrap()
        .permissions()
        .mode()
        & 0o7777;
    assert_eq!(mode, 0o640);
    assert!(leftovers(&f.checkout).is_empty());
}

#[test]
fn a_file_changed_since_it_was_read_is_a_conflict_and_is_not_written() {
    let f = fixture("a.txt", "old");
    let read = revision_of(b"old");
    fs::write(f.checkout.join("a.txt"), "theirs").unwrap();
    let error = save(f.root.dir(), Path::new("a.txt"), b"mine", &read).unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(
        error.actual_revision.as_deref(),
        Some(revision_of(b"theirs").as_str())
    );
    assert_eq!(
        fs::read_to_string(f.checkout.join("a.txt")).unwrap(),
        "theirs"
    );
}

/// The window the check cannot close: another writer finishes a write after
/// the check and before the exchange. Their version must survive.
#[test]
fn a_write_that_lands_between_the_check_and_the_exchange_is_kept() {
    let f = fixture("a.txt", "old");
    let path = f.checkout.join("a.txt");
    let error = save_observed(
        f.root.dir(),
        Path::new("a.txt"),
        b"mine",
        &revision_of(b"old"),
        &mut |stage, _, _| {
            assert_eq!(stage, SaveStage::BeforeExchange);
            fs::write(&path, "theirs, written in place").unwrap();
        },
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "theirs, written in place"
    );
    assert!(leftovers(&f.checkout).is_empty());
}

/// Editors that save by rename replace the inode rather than writing it.
#[test]
fn a_file_replaced_by_rename_in_the_window_is_kept() {
    let f = fixture("a.txt", "old");
    let path = f.checkout.join("a.txt");
    let error = save_observed(
        f.root.dir(),
        Path::new("a.txt"),
        b"mine",
        &revision_of(b"old"),
        &mut |_, _, _| {
            let staged = f.checkout.join("other-editor.tmp");
            fs::write(&staged, "theirs, renamed over").unwrap();
            fs::rename(&staged, &path).unwrap();
        },
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs, renamed over");
}

/// A deleted file in the window is a conflict too: the save does not bring
/// back a file someone removed, and it does not report success.
#[test]
fn a_file_deleted_in_the_window_is_not_recreated() {
    let f = fixture("a.txt", "old");
    let path = f.checkout.join("a.txt");
    let error = save_observed(
        f.root.dir(),
        Path::new("a.txt"),
        b"mine",
        &revision_of(b"old"),
        &mut |_, _, _| fs::remove_file(&path).unwrap(),
    )
    .unwrap_err();
    assert_ne!(error.code, ErrorCode::InvalidRequest);
    assert!(!path.exists(), "a deleted file must not reappear");
    assert!(leftovers(&f.checkout).is_empty());
}

/// The checkout is moved away and another folder takes its path while the
/// save runs: the opened handle still names the verified file, so the write
/// lands there and nothing is written into the impostor.
#[test]
fn a_checkout_replaced_during_the_save_does_not_redirect_it() {
    let f = fixture("a.txt", "old");
    let moved = f.checkout.with_file_name("moved");
    let result = save_observed(
        f.root.dir(),
        Path::new("a.txt"),
        b"mine",
        &revision_of(b"old"),
        &mut |_, _, _| {
            fs::rename(&f.checkout, &moved).unwrap();
            fs::create_dir(&f.checkout).unwrap();
            fs::write(f.checkout.join("a.txt"), "impostor").unwrap();
        },
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(fs::read_to_string(moved.join("a.txt")).unwrap(), "mine");
    assert_eq!(
        fs::read_to_string(f.checkout.join("a.txt")).unwrap(),
        "impostor"
    );
    assert_eq!(
        Root::open_pinned(&f.checkout, f.root.identity())
            .unwrap_err()
            .code,
        ErrorCode::RootReplaced
    );
}

#[test]
fn a_link_inside_the_checkout_saves_its_target_and_a_link_out_of_it_is_refused() {
    let f = fixture("target.txt", "old");
    symlink("target.txt", f.checkout.join("link.txt")).unwrap();
    save(
        f.root.dir(),
        Path::new("link.txt"),
        b"through the link",
        &revision_of(b"old"),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(f.checkout.join("target.txt")).unwrap(),
        "through the link"
    );
    assert!(
        fs::symlink_metadata(f.checkout.join("link.txt"))
            .unwrap()
            .file_type()
            .is_symlink()
    );

    let outside = f.checkout.parent().unwrap().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    symlink(&outside, f.checkout.join("escape")).unwrap();
    symlink(
        outside.join("secret.txt"),
        f.checkout.join("secret-link.txt"),
    )
    .unwrap();
    for path in ["escape/secret.txt", "secret-link.txt"] {
        let error = save(f.root.dir(), Path::new(path), b"x", &revision_of(b"secret")).unwrap_err();
        assert!(
            matches!(
                error.code,
                ErrorCode::OutsideRoot | ErrorCode::PermissionDenied
            ),
            "{path}: {error:?}"
        );
    }
    assert_eq!(
        fs::read_to_string(outside.join("secret.txt")).unwrap(),
        "secret"
    );
    assert!(document::open(f.root.dir(), Path::new("escape/secret.txt")).is_err());
}

#[test]
fn read_only_and_hard_linked_files_are_refused_unchanged() {
    let f = fixture("a.txt", "old");
    let path = f.checkout.join("a.txt");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let error = save(f.root.dir(), Path::new("a.txt"), b"x", &revision_of(b"old")).unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert_eq!(fs::read_to_string(&path).unwrap(), "old");

    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    fs::hard_link(&path, f.checkout.join("twin.txt")).unwrap();
    let error = save(f.root.dir(), Path::new("a.txt"), b"x", &revision_of(b"old")).unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(fs::read_to_string(&path).unwrap(), "old");
}

#[test]
fn saving_what_is_already_there_writes_nothing() {
    let f = fixture("a.txt", "same");
    let before = fs::metadata(f.checkout.join("a.txt")).unwrap();
    let saved = save(
        f.root.dir(),
        Path::new("a.txt"),
        b"same",
        &revision_of(b"same"),
    )
    .unwrap();
    assert_eq!(saved.revision, revision_of(b"same"));
    let after = fs::metadata(f.checkout.join("a.txt")).unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_eq!(before.ino(), after.ino());
}

/// A save repeated after its answer was lost finds its own bytes there.
#[test]
fn a_file_that_already_holds_the_draft_is_saved_whatever_revision_was_expected() {
    let f = fixture("a.txt", "mine");
    let saved = save(
        f.root.dir(),
        Path::new("a.txt"),
        b"mine",
        &revision_of(b"old"),
    )
    .unwrap();
    assert_eq!(saved.revision, revision_of(b"mine"));
}

/// Settling a save whose answer was lost reads the revision after that save
/// has finished, even from another handle, as a new helper would.
#[test]
fn a_revision_read_waits_for_a_save_in_progress_in_the_folder() {
    let f = fixture("a.txt", "old");
    let checkout = f.checkout.clone();
    let mut reader = None;
    save_observed(
        f.root.dir(),
        Path::new("a.txt"),
        b"mine",
        &revision_of(b"old"),
        &mut |_, _, _| {
            let checkout = checkout.clone();
            reader = Some(std::thread::spawn(move || {
                let root = Root::open(&checkout).unwrap();
                hide_host::save::current_revision(root.dir(), Path::new("a.txt")).unwrap()
            }));
            std::thread::sleep(std::time::Duration::from_millis(200));
        },
    )
    .unwrap();
    assert_eq!(reader.unwrap().join().unwrap(), revision_of(b"mine"));
}
