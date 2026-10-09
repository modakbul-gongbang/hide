use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn native_override_registry_owns_raw_reads_and_matches_the_example() {
    let keys: BTreeSet<_> = hide_session::environment::REGISTRY
        .iter()
        .map(|entry| entry.key)
        .collect();
    let example: BTreeSet<_> = include_str!("../.env.example")
        .lines()
        .filter_map(|line| line.strip_prefix("# ")?.split_once('=').map(|(key, _)| key))
        .collect();
    assert_eq!(keys, example);
    fn inspect(folder: &Path) {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                inspect(&path);
            } else if path.extension().is_some_and(|extension| extension == "rs")
                && path.file_name().unwrap() != "environment.rs"
            {
                let source = std::fs::read_to_string(&path).unwrap();
                assert!(
                    !source.contains("std::env::var(") && !source.contains("std::env::var_os("),
                    "raw native environment read outside registry: {}",
                    path.display()
                );
            }
        }
    }
    inspect(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
}
