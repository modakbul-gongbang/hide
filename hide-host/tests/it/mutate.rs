//! The Explorer change contract (PRD S5.5 B16-B18): changes stay inside the
//! opened checkout, never replace an existing item, and a trash moves only
//! the item the operator confirmed.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};

use hide_host::mutate::{create, move_into, rename, trash};
use hide_host::{ErrorCode, Root};

struct Fixture {
    outer: tempfile::TempDir,
    checkout: PathBuf,
    root: Root,
}

fn fixture() -> Fixture {
    let outer = tempfile::tempdir().unwrap();
    let checkout = outer.path().join("checkout");
    fs::create_dir_all(checkout.join("src")).unwrap();
    fs::create_dir(checkout.join("dest")).unwrap();
    fs::write(checkout.join("src/a.txt"), "a").unwrap();
    fs::write(checkout.join("dest/a.txt"), "kept").unwrap();
    fs::write(checkout.join("src/b.txt"), "b").unwrap();
    let root = Root::open(&checkout).unwrap();
    Fixture {
        outer,
        checkout,
        root,
    }
}

#[test]
fn a_creation_never_replaces_an_existing_item() {
    let f = fixture();
    create(f.root.dir(), Path::new("src"), "new.txt", false).unwrap();
    create(f.root.dir(), Path::new(""), "folder", true).unwrap();
    assert!(f.checkout.join("src/new.txt").is_file());
    assert!(f.checkout.join("folder").is_dir());

    let refused = create(f.root.dir(), Path::new("src"), "a.txt", false).unwrap_err();
    assert_eq!(refused.code, ErrorCode::AlreadyExists);
    assert_eq!(refused.message, "a.txt already exists in src");
    assert_eq!(
        fs::read_to_string(f.checkout.join("src/a.txt")).unwrap(),
        "a"
    );
    assert_eq!(
        create(f.root.dir(), Path::new(""), "src", true)
            .unwrap_err()
            .code,
        ErrorCode::AlreadyExists
    );
    for name in ["", "a/b", "..", "."] {
        assert_eq!(
            create(f.root.dir(), Path::new("src"), name, false)
                .unwrap_err()
                .code,
            ErrorCode::InvalidPath,
            "{name:?}"
        );
    }
}

#[test]
fn a_rename_or_move_refuses_a_name_that_is_taken_and_leaves_both_items() {
    let f = fixture();
    let clash = rename(f.root.dir(), Path::new("src/a.txt"), "b.txt").unwrap_err();
    assert_eq!(clash.code, ErrorCode::AlreadyExists);
    let clash = move_into(f.root.dir(), Path::new("src/a.txt"), Path::new("dest")).unwrap_err();
    assert_eq!(clash.code, ErrorCode::AlreadyExists);
    assert_eq!(
        fs::read_to_string(f.checkout.join("src/a.txt")).unwrap(),
        "a"
    );
    assert_eq!(
        fs::read_to_string(f.checkout.join("dest/a.txt")).unwrap(),
        "kept"
    );
    assert_eq!(
        fs::read_to_string(f.checkout.join("src/b.txt")).unwrap(),
        "b"
    );

    rename(f.root.dir(), Path::new("src/a.txt"), "c.txt").unwrap();
    move_into(f.root.dir(), Path::new("src/c.txt"), Path::new("dest")).unwrap();
    assert_eq!(
        fs::read_to_string(f.checkout.join("dest/c.txt")).unwrap(),
        "a"
    );
    move_into(f.root.dir(), Path::new("dest/c.txt"), Path::new("")).unwrap();
    assert!(f.checkout.join("c.txt").is_file());

    assert_eq!(
        move_into(f.root.dir(), Path::new("src"), Path::new("src"))
            .unwrap_err()
            .message,
        "A folder cannot be moved into itself"
    );
    let gone = rename(f.root.dir(), Path::new("src/missing.txt"), "x").unwrap_err();
    assert_eq!(
        (gone.code, gone.message.as_str()),
        (ErrorCode::NotFound, "missing.txt no longer exists")
    );
}

#[test]
fn a_link_out_of_the_checkout_takes_no_change_there() {
    let f = fixture();
    let outside = f.outer.path().join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, f.checkout.join("escape")).unwrap();
    symlink("../../outside", f.checkout.join("src/relative-escape")).unwrap();

    for folder in ["escape", "src/relative-escape"] {
        assert!(
            create(f.root.dir(), Path::new(folder), "planted", false).is_err(),
            "{folder}"
        );
        assert!(
            move_into(f.root.dir(), Path::new("src/b.txt"), Path::new(folder)).is_err(),
            "{folder}"
        );
    }
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    assert!(f.checkout.join("src/b.txt").is_file());

    // The link itself is an item of the checkout: renaming it renames the
    // link and leaves its target alone.
    rename(f.root.dir(), Path::new("escape"), "renamed-link").unwrap();
    assert!(
        fs::symlink_metadata(f.checkout.join("renamed-link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(outside.is_dir());
}

#[test]
fn a_trash_refuses_an_item_replaced_since_it_was_confirmed() {
    let f = fixture();
    let confirmed = fs::symlink_metadata(f.checkout.join("src/a.txt"))
        .unwrap()
        .ino();
    fs::rename(f.checkout.join("src/a.txt"), f.checkout.join("src/old.txt")).unwrap();
    fs::write(f.checkout.join("src/a.txt"), "replacement").unwrap();

    let refused = trash(&f.root, Path::new("src/a.txt"), Some(confirmed)).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Conflict);
    assert_eq!(
        refused.message,
        "a.txt changed while the prompt was open; nothing was moved"
    );
    assert_eq!(
        fs::read_to_string(f.checkout.join("src/a.txt")).unwrap(),
        "replacement"
    );
    assert_eq!(
        fs::read_to_string(f.checkout.join("src/old.txt")).unwrap(),
        "a"
    );

    let gone = trash(&f.root, Path::new("src/missing.txt"), None).unwrap_err();
    assert_eq!(gone.code, ErrorCode::NotFound);
    assert_eq!(
        trash(&f.root, Path::new(""), None).unwrap_err().code,
        ErrorCode::InvalidPath,
        "the checkout root itself is not an item"
    );
}
