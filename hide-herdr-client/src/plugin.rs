//! The plugin registry entry the kit reads, from `plugin.list` and from
//! Herdr's offline `plugins.json`, which share one shape.
//!
//! This is the one response type outside `herdr-core`'s `wire` module, written
//! by hand because `hide-kit` cannot depend on the core: it keeps the fields
//! the schema requires and the source kind, and `entry_matches_the_pinned_schema`
//! fails when the pinned schema's entry stops agreeing with it.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct InstalledPlugin {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub manifest_path: String,
    pub plugin_root: String,
    pub enabled: bool,
    #[serde(default)]
    pub source: PluginSource,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct PluginSource {
    #[serde(default)]
    pub kind: PluginSourceKind,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginSourceKind {
    #[default]
    Local,
    Github,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn pinned(name: &str) -> Value {
        let schema: Value = serde_json::from_str(crate::HERDR_API_SCHEMA_JSON).expect("schema");
        schema["schemas"]["success_response"]["$defs"][name].clone()
    }

    #[test]
    fn entry_matches_the_pinned_schema() {
        let entry = pinned("InstalledPluginInfo");
        let required: Vec<&str> = entry["required"]
            .as_array()
            .expect("required fields")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(
            required,
            [
                "plugin_id",
                "name",
                "version",
                "manifest_path",
                "plugin_root",
                "enabled"
            ]
        );
        assert_eq!(
            entry["properties"]["source"]["default"],
            json!({"kind": "local"})
        );
        let source = pinned("PluginSourceInfo");
        assert_eq!(source["properties"]["kind"]["default"], "local");
        assert_eq!(
            pinned("PluginSourceKind")["enum"],
            json!(["local", "github"])
        );
    }

    #[test]
    fn a_registry_entry_reads_with_and_without_a_source() {
        let linked: InstalledPlugin = serde_json::from_value(json!({
            "plugin_id": "p", "name": "n", "version": "1", "manifest_path": "/m",
            "plugin_root": "/r", "enabled": true, "warnings": []
        }))
        .expect("an entry with no source is a linked folder");
        assert_eq!(linked.source.kind, PluginSourceKind::Local);
        let managed: InstalledPlugin = serde_json::from_value(json!({
            "plugin_id": "p", "name": "n", "version": "1", "manifest_path": "/m",
            "plugin_root": "/r", "enabled": true, "source": {"kind": "github"}
        }))
        .expect("a GitHub entry");
        assert_eq!(managed.source.kind, PluginSourceKind::Github);
        assert!(
            serde_json::from_value::<InstalledPlugin>(json!({"plugin_id": "p"})).is_err(),
            "a registry entry missing required fields stays unreadable"
        );
    }
}
