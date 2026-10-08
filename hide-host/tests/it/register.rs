//! Which folders a host lets be registered as projects (`Call::Registrable`):
//! only a folder inside its own home, named by the project it belongs to.

use std::path::Path;
use std::process::Command;

use hide_host::ErrorCode;
use hide_host::register::check;

#[test]
fn a_folder_inside_home_is_registered_as_its_project() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let folder = home.join("notes");
    std::fs::create_dir(&folder).unwrap();
    let repository = home.join("repo");
    std::fs::create_dir_all(repository.join("src")).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&repository)
            .status()
            .unwrap()
            .success()
    );

    let plain = check(&folder, &home).unwrap();
    assert_eq!(plain.root, folder.to_string_lossy());
    assert!(!plain.is_git);
    let inside = check(&repository.join("src"), &home).unwrap();
    assert_eq!(inside.root, repository.to_string_lossy());
    assert!(inside.is_git);
}

#[test]
fn home_itself_a_folder_outside_it_and_a_file_are_refused() {
    let home = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let file = home.path().join("file.txt");
    std::fs::write(&file, "x").unwrap();

    let code = |path: &Path| check(path, home.path()).unwrap_err().code;
    assert_eq!(code(home.path()), ErrorCode::OutsideRoot);
    assert_eq!(code(elsewhere.path()), ErrorCode::OutsideRoot);
    assert_eq!(code(&file), ErrorCode::NotADirectory);
    assert_eq!(code(&home.path().join("missing")), ErrorCode::NotFound);
}
