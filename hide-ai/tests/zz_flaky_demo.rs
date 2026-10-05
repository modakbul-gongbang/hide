// TEMPORARY: proves the nextest retry and the flaky report on this pull request. Removed before merge.
#[test]
fn flaky_demo_fails_the_first_attempt_and_passes_the_retry() {
    let marker = std::env::temp_dir().join("hide-flaky-demo-marker");
    if !marker.exists() {
        std::fs::write(&marker, "ran").unwrap();
        panic!("first attempt fails on purpose");
    }
}
