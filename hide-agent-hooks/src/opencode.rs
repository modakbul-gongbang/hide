//! Hide's OpenCode plugin: the one file of Hide's in OpenCode's global
//! configuration, `~/.config/opencode/plugins/hide.js`, owned the way every
//! script file of Hide's is (`crate::plugin`).
//!
//! It asks the helper (`hide-agent-hooks opencode <operation>`) on each root
//! prompt, each shell or `question` tool call and each change of a subagent's
//! state.

use crate::plugin::PluginFile;

/// OpenCode's global configuration folder as Herdr's integration finds it,
/// `~/.config/opencode` on every system, so Hide's plugin sits beside Herdr's.
/// OpenCode itself reads `$XDG_CONFIG_HOME/opencode` when that is set; such a
/// machine loads neither plugin from here (known limitation, shared with Herdr).
pub const PLUGIN: PluginFile = PluginFile {
    agent: "OpenCode",
    noun: "plugin",
    source_name: "hide-opencode-plugin",
    version: 1,
    config: &[".config", "opencode"],
    folder: "plugins",
    file_name: "hide.js",
    template: include_str!("opencode/plugin.js"),
    constants: &[],
};
