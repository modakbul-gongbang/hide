//! The stamps a device Explorer's watch compares (PRD S5.5 B43): a folder's
//! stamp moves when its list of names changes or another folder takes its
//! path, stays put otherwise, and never reads a folder outside the checkout.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;

use hide_host::list::{MAX_STAMPED_FOLDERS, stamps};
use hide_host::{ErrorCode, Root};

#[test]
fn a_folders_stamp_moves_only_when_its_names_change_or_it_is_replaced() {
    let outer = tempfile::tempdir().unwrap();
    let checkout = outer.path().join("checkout");
    fs::create_dir_all(checkout.join("src")).unwrap();
    fs::create_dir(outer.path().join("outside")).unwrap();
    symlink(outer.path().join("outside"), checkout.join("escape")).unwrap();
    let root = Root::open(&checkout).unwrap();
    let folders = [
        "".to_owned(),
        "src".to_owned(),
        "escape".to_owned(),
        "gone".to_owned(),
    ];

    let first = stamps(root.dir(), &folders).unwrap();
    assert!(first[0].is_some() && first[1].is_some());
    assert_eq!(
        first[2], None,
        "a link that leaves the checkout is not read"
    );
    assert_eq!(first[3], None);
    assert_eq!(
        stamps(root.dir(), &folders).unwrap(),
        first,
        "nothing changed"
    );

    fs::write(checkout.join("src/new.txt"), "x").unwrap();
    let added = stamps(root.dir(), &folders).unwrap();
    assert_eq!(
        added[0], first[0],
        "a change inside src leaves the root's names alone"
    );
    assert_ne!(added[1], first[1]);

    fs::rename(checkout.join("src"), checkout.join("old")).unwrap();
    fs::create_dir(checkout.join("src")).unwrap();
    let replaced = stamps(root.dir(), &folders).unwrap();
    assert_ne!(
        replaced[1], added[1],
        "another folder at the path is another stamp"
    );

    let error = stamps(root.dir(), &vec![String::new(); MAX_STAMPED_FOLDERS + 1]).unwrap_err();
    assert_eq!(error.code, ErrorCode::TooLarge);
    let error = stamps(root.dir(), &["../outside".to_owned()]).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidPath);
}
