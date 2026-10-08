//! The shared persistence API observes external changes before replacement.
//! Real files fix the order without a pause seam in the public installer.

use super::*;

#[test]
fn an_external_edit_observed_after_read_refuses_replacement_and_keeps_its_bytes() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("hooks.json");
    fs::write(&path, r#"{"hooks":{}}"#).unwrap();
    let mut document = read_document(&path).unwrap().unwrap();
    document["hooks"]["Stop"] = serde_json::json!([{ "hooks": [{"command":"owned"}] }]);
    let external = " \r\n{ \"operator\" : \"new edit\", \"hooks\" : {} }\r\n";
    fs::write(&path, external).unwrap();

    assert!(matches!(
        write_document(&path, &document),
        Err(InstallFailure::NotWritable { .. })
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
}

#[test]
fn a_retargeted_settings_link_is_refused_even_when_both_files_have_equal_bytes() {
    use hide_platform::fs::link;

    let home = tempfile::tempdir().unwrap();
    let first = home.path().join("first.json");
    let second = home.path().join("second.json");
    let path = home.path().join("hooks.json");
    let source = r#"{ "hooks" : {} }"#;
    fs::write(&first, source).unwrap();
    fs::write(&second, source).unwrap();
    match link::create_link(&first, &path) {
        Ok(()) => {}
        Err(error) if link::needs_privilege(&error) => return,
        Err(error) => panic!("{error}"),
    }
    let mut document = read_document(&path).unwrap().unwrap();
    document["hooks"]["Stop"] = serde_json::json!([{ "hooks": [{"command":"owned"}] }]);
    fs::remove_file(&path).unwrap();
    link::create_link(&second, &path).unwrap();

    assert!(matches!(
        write_document(&path, &document),
        Err(InstallFailure::NotWritable { .. })
    ));
    assert!(link::is_link_to(&path, &second));
    assert_eq!(fs::read_to_string(first).unwrap(), source);
    assert_eq!(fs::read_to_string(second).unwrap(), source);
}
