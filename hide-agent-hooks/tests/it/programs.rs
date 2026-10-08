//! The helper as a test runs it against the hook's budget (`stand_ins`).

use std::path::Path;

use crate::stand_ins;

/// The helper cargo built, ready to run, with the `hide` it asks when cargo
/// built one beside it. A test that needs the helper beside a stand-in puts
/// a hard link to this file there ([`stand_ins::place`]).
pub fn hook() -> &'static Path {
    let hook = Path::new(env!("CARGO_BIN_EXE_hide-agent-hooks"));
    stand_ins::ready(hook);
    let sibling = hook.with_file_name(format!("hide{}", std::env::consts::EXE_SUFFIX));
    if sibling.exists() {
        stand_ins::ready(&sibling);
    }
    hook
}
