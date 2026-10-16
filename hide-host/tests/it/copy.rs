//! The copy contract of the Explorer: an item copied into a folder, and a drop
//! from the file manager written into one, stay inside the opened checkout,
//! never replace an existing item, appear whole or not at all, and refuse
//! what they cannot copy faithfully.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use hide_host::copy::{copy_into, import};
use hide_host::{ErrorCode, Root};
use hide_node_link::mutate::{
    COPY_MAX_ITEMS, IMPORT_MAX_FILE_BYTES, IMPORT_MAX_ITEMS, ImportEntry,
};

struct Fixture {
    _outer: tempfile::TempDir,
    checkout: PathBuf,
    root: Root,
}

fn fixture() -> Fixture {
    let outer = tempfile::tempdir().unwrap();
    let checkout = outer.path().join("checkout");
    fs::create_dir_all(checkout.join("src/nested")).unwrap();
    fs::create_dir(checkout.join("dest")).unwrap();
    fs::write(checkout.join("src/a.txt"), "a").unwrap();
    fs::write(checkout.join("src/nested/deep.txt"), "deep").unwrap();
    fs::write(checkout.join("dest/a.txt"), "kept").unwrap();
    let root = Root::open(&checkout).unwrap();
    Fixture {
        _outer: outer,
        checkout,
        root,
    }
}

fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn file(path: &str, bytes: &[u8]) -> ImportEntry {
    ImportEntry {
        path: path.to_owned(),
        directory: false,
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

fn folder(path: &str) -> ImportEntry {
    ImportEntry {
        path: path.to_owned(),
        directory: true,
        data: String::new(),
    }
}

#[test]
fn a_copy_keeps_the_source_and_takes_the_next_free_copy_name() {
    let f = fixture();
    let first = copy_into(f.root.dir(), Path::new("src/a.txt"), Path::new("src")).unwrap();
    assert_eq!(first.name, "a copy.txt");
    let second = copy_into(f.root.dir(), Path::new("src/a.txt"), Path::new("src")).unwrap();
    assert_eq!(second.name, "a copy 2.txt");
    assert_eq!(
        fs::read_to_string(f.checkout.join("src/a.txt")).unwrap(),
        "a"
    );
    assert_eq!(
        fs::read_to_string(f.checkout.join("src/a copy 2.txt")).unwrap(),
        "a"
    );

    // Into another folder a free name is kept; a taken one is not replaced.
    let moved = copy_into(
        f.root.dir(),
        Path::new("src/nested/deep.txt"),
        Path::new("dest"),
    )
    .unwrap();
    assert_eq!(moved.name, "deep.txt");
    let clash = copy_into(f.root.dir(), Path::new("src/a.txt"), Path::new("dest")).unwrap();
    assert_eq!(clash.name, "a copy.txt");
    assert_eq!(
        fs::read_to_string(f.checkout.join("dest/a.txt")).unwrap(),
        "kept"
    );
}

#[test]
fn a_folder_is_copied_with_everything_under_it_and_leaves_no_working_folder() {
    let f = fixture();
    let copied = copy_into(f.root.dir(), Path::new("src"), Path::new("dest")).unwrap();
    assert_eq!(copied.name, "src");
    assert_eq!(
        fs::read_to_string(f.checkout.join("dest/src/nested/deep.txt")).unwrap(),
        "deep"
    );
    assert_eq!(names_in(&f.checkout.join("dest")), ["a.txt", "src"]);

    let again = copy_into(f.root.dir(), Path::new("src"), Path::new("dest")).unwrap();
    assert_eq!(again.name, "src copy");
    assert_eq!(
        names_in(&f.checkout.join("dest")),
        ["a.txt", "src", "src copy"]
    );
}

#[test]
fn a_folder_is_not_copied_into_itself_or_below_itself() {
    let f = fixture();
    for destination in ["src", "src/nested"] {
        let refused =
            copy_into(f.root.dir(), Path::new("src"), Path::new(destination)).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidPath, "{destination}");
    }
    assert_eq!(names_in(&f.checkout.join("src")), ["a.txt", "nested"]);
}

#[test]
fn a_missing_source_or_destination_is_refused_and_leaves_nothing() {
    let f = fixture();
    assert_eq!(
        copy_into(f.root.dir(), Path::new("src/gone.txt"), Path::new("dest"))
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        copy_into(f.root.dir(), Path::new("src/a.txt"), Path::new("nowhere"))
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(names_in(&f.checkout.join("dest")), ["a.txt"]);
}

#[test]
fn a_copy_past_the_item_cap_is_refused_before_anything_is_written() {
    let f = fixture();
    let many = f.checkout.join("many");
    fs::create_dir(&many).unwrap();
    for index in 0..=COPY_MAX_ITEMS {
        fs::write(many.join(format!("{index}")), "").unwrap();
    }
    let refused = copy_into(f.root.dir(), Path::new("many"), Path::new("dest")).unwrap_err();
    assert_eq!(refused.code, ErrorCode::TooLarge);
    assert!(
        refused.message.contains("nothing was copied"),
        "{}",
        refused.message
    );
    assert_eq!(names_in(&f.checkout.join("dest")), ["a.txt"]);
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn a_link_is_refused_whole_and_the_destination_stays_as_it_was() {
        let f = fixture();
        symlink("/etc/hosts", f.checkout.join("src/outside")).unwrap();
        let refused = copy_into(f.root.dir(), Path::new("src"), Path::new("dest")).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Unsupported);
        assert!(refused.message.contains("outside"), "{}", refused.message);
        assert_eq!(names_in(&f.checkout.join("dest")), ["a.txt"]);
        let direct =
            copy_into(f.root.dir(), Path::new("src/outside"), Path::new("dest")).unwrap_err();
        assert_eq!(direct.code, ErrorCode::Unsupported);
    }

    #[test]
    fn a_copy_keeps_the_mode_of_a_file() {
        let f = fixture();
        let script = f.checkout.join("src/run.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        copy_into(f.root.dir(), Path::new("src/run.sh"), Path::new("dest")).unwrap();
        let mode = fs::metadata(f.checkout.join("dest/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}

#[test]
fn a_drop_lands_each_item_under_its_name_and_a_taken_name_becomes_copy() {
    let f = fixture();
    let imported = import(
        f.root.dir(),
        Path::new("dest"),
        &[
            file("a.txt", b"dropped"),
            folder("photos"),
            file("photos/one.png", b"png"),
            folder("photos/raw"),
            file("photos/raw/two.png", b"raw"),
        ],
    )
    .unwrap();
    assert_eq!(imported.names, ["a copy.txt", "photos"]);
    assert_eq!(
        fs::read_to_string(f.checkout.join("dest/a.txt")).unwrap(),
        "kept"
    );
    assert_eq!(
        fs::read_to_string(f.checkout.join("dest/a copy.txt")).unwrap(),
        "dropped"
    );
    assert_eq!(
        fs::read_to_string(f.checkout.join("dest/photos/raw/two.png")).unwrap(),
        "raw"
    );
    assert_eq!(
        names_in(&f.checkout.join("dest")),
        ["a copy.txt", "a.txt", "photos"]
    );
}

#[test]
fn a_drop_that_cannot_be_written_leaves_nothing_in_the_folder() {
    let f = fixture();
    let before = names_in(&f.checkout.join("dest"));
    // The second file's folder was never listed.
    let orphan = import(
        f.root.dir(),
        Path::new("dest"),
        &[file("ok.txt", b"ok"), file("missing/inner.txt", b"x")],
    )
    .unwrap_err();
    assert_eq!(orphan.code, ErrorCode::InvalidPath);
    // Damaged bytes arrive after the first file was written.
    let damaged = import(
        f.root.dir(),
        Path::new("dest"),
        &[
            file("ok.txt", b"ok"),
            ImportEntry {
                path: "bad.txt".to_owned(),
                directory: false,
                data: "***".to_owned(),
            },
        ],
    )
    .unwrap_err();
    assert_eq!(damaged.code, ErrorCode::InvalidRequest);
    assert_eq!(names_in(&f.checkout.join("dest")), before);
}

#[test]
fn a_drop_refuses_paths_that_leave_the_folder_and_shapes_the_page_never_sends() {
    let f = fixture();
    for path in ["../escape.txt", "/etc/x", "a/../b.txt", "./c.txt", ""] {
        let refused = import(f.root.dir(), Path::new("dest"), &[file(path, b"x")]).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidPath, "{path:?}");
    }
    let twice = import(
        f.root.dir(),
        Path::new("dest"),
        &[file("x.txt", b"1"), file("x.txt", b"2")],
    )
    .unwrap_err();
    assert_eq!(twice.code, ErrorCode::InvalidPath);
    assert_eq!(
        import(f.root.dir(), Path::new("dest"), &[])
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(names_in(&f.checkout.join("dest")), ["a.txt"]);
}

#[test]
fn a_drop_past_a_cap_is_refused_whole() {
    let f = fixture();
    let too_many: Vec<ImportEntry> = (0..=IMPORT_MAX_ITEMS)
        .map(|n| file(&format!("f{n}"), b""))
        .collect();
    assert_eq!(
        import(f.root.dir(), Path::new("dest"), &too_many)
            .unwrap_err()
            .code,
        ErrorCode::TooLarge
    );
    let big = vec![0u8; IMPORT_MAX_FILE_BYTES as usize + 1];
    let refused = import(f.root.dir(), Path::new("dest"), &[file("big.bin", &big)]).unwrap_err();
    assert_eq!(refused.code, ErrorCode::TooLarge);
    assert_eq!(names_in(&f.checkout.join("dest")), ["a.txt"]);
}
