//! Native Cursor overrides are refusal inputs, never reader authority.
//! A node captures and validates its own process values at startup; nothing
//! here certifies the environment a pane's shell will later give Cursor.

use std::ffi::OsString;
use std::path::{Component, PathBuf};
use std::sync::OnceLock;

pub struct VariableSpec {
    pub key: &'static str,
    pub required: bool,
    pub shape: &'static str,
    pub absent_behavior: &'static str,
}

pub const REGISTRY: [VariableSpec; 2] = [
    VariableSpec {
        key: "CURSOR_CONFIG_DIR",
        required: false,
        shape: "absolute UTF-8 directory path, at most 4096 bytes, without controls or parent components",
        absent_behavior: "Cursor selects XDG_CONFIG_HOME/cursor, or the node's HOME/.cursor when both overrides are absent or blank",
    },
    VariableSpec {
        key: "XDG_CONFIG_HOME",
        required: false,
        shape: "absolute UTF-8 directory path, at most 4096 bytes, without controls or parent components",
        absent_behavior: "Cursor selects HOME/.cursor when CURSOR_CONFIG_DIR is also absent or blank",
    },
];

#[derive(Clone, Debug)]
pub(crate) struct CursorRoots {
    pub config: Option<PathBuf>,
    pub xdg: Option<PathBuf>,
}

fn validated(value: Option<OsString>) -> Result<Option<PathBuf>, &'static str> {
    let Some(value) = value else { return Ok(None) };
    let text = value.to_str().ok_or("cursor_launch_environment_invalid")?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(text);
    if text.len() > 4096
        || text.chars().any(char::is_control)
        || !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("cursor_launch_environment_invalid");
    }
    Ok(Some(path))
}

fn read(variables: &dyn Fn(&str) -> Option<OsString>) -> Result<CursorRoots, &'static str> {
    // Inspect every optional key once, including a lower-priority override.
    // Invalid values disable only Cursor lifecycle effects, not node startup.
    let config = validated(variables(REGISTRY[0].key));
    let xdg = validated(variables(REGISTRY[1].key));
    Ok(CursorRoots {
        config: config?,
        xdg: xdg?.map(|path| path.join("cursor")),
    })
}

static CURSOR_ROOTS: OnceLock<Result<CursorRoots, &'static str>> = OnceLock::new();

/// Validate before a node accepts requests. Optional invalid values are
/// retained as a feature-local refusal, without publishing their values.
pub fn initialize() {
    let _ = cursor_roots();
}

pub(crate) fn cursor_roots() -> Result<&'static CursorRoots, &'static str> {
    CURSOR_ROOTS
        .get_or_init(|| read(&|key| std::env::var_os(key)))
        .as_ref()
        .map_err(|reason| *reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_native_overrides_validate_without_logging_values() {
        assert!(read(&|_| None).unwrap().config.is_none());
        assert!(read(&|_| Some(" \t".into())).unwrap().xdg.is_none());
        for value in ["relative", "secret\nvalue", "/absolute/../parent"] {
            assert_eq!(
                read(&|_| Some(value.into())).unwrap_err(),
                "cursor_launch_environment_invalid"
            );
        }
        let absolute = std::env::current_dir().unwrap();
        let roots = read(&|_| Some(absolute.clone().into_os_string())).unwrap();
        assert_eq!(roots.config.as_ref(), Some(&absolute));
        assert_eq!(roots.xdg, Some(absolute.join("cursor")));
        assert!(REGISTRY.iter().all(|entry| !entry.required
            && !entry.shape.is_empty()
            && !entry.absent_behavior.is_empty()));
    }
}
