//! The helper as a test runs it against the hook's budget (`stand_ins`).

use std::path::Path;

#[cfg(not(target_os = "macos"))]
use crate::stand_ins;

/// The helper cargo built. Unix fixtures place it with `stand_ins::place`
/// before starting their deadline; macOS admits that private signed copy,
/// rather than starting the linker-signed build output first.
pub fn hook() -> &'static Path {
    let hook = Path::new(env!("CARGO_BIN_EXE_hide-agent-hooks"));
    #[cfg(not(target_os = "macos"))]
    {
        stand_ins::ready(hook);
        let sibling = hook.with_file_name(format!("hide{}", std::env::consts::EXE_SUFFIX));
        if sibling.exists() {
            stand_ins::ready(&sibling);
        }
    }
    hook
}
