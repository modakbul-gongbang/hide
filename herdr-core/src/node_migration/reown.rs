//! The owner change a core move makes to a copy of the state folder (PRD
//! core-host-node-move D-03, B4, B6).
//!
//! A folder names its owner's machine by leaving keys unqualified and every
//! other machine by an id: a dialed device by the registration id the
//! operator gave it (`mini`, `remote:mini:pane:w1:p1`), a node that links in
//! by its node id (`remote:<node>:pane:w1:p1`). When the core moves, the
//! machine taking it becomes the owner, so its keys lose their qualifier,
//! and the machine giving it up gains one. `reown` makes that change on a
//! staging copy, through the same `KEYS` table and marker as the layer-1
//! conversion: each row says how its value moves (`Moves`), and a key that
//! still names the new owner by its old id afterwards stops the move with its
//! file and place (engineering principle 4). The source folder is never
//! written. A move back is the same change with the two machines swapped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{
    CORE_STATE, DELIVERY_LEDGER, KEYS, LABELS, LINKS, LOCAL_ISSUES, MARKER_FILE, MARKER_VERSION,
    Marker, Moves, Refusal, WORKSPACE_VIEWS, read_json, read_marker, write,
};
use crate::node::NodeId;

/// The two machines of a move, by the names the folder uses for them before
/// and after it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OwnerChange {
    /// The node whose core wrote the folder; `node.json` must name it.
    pub old_owner: String,
    /// The id the result gives the old owner: its node id when it links in to
    /// the new core, the registration id it was dialed by when the new core
    /// dials it.
    pub old_owner_as: String,
    /// The node taking the core.
    pub new_owner: String,
    /// The id the folder gives the new owner before the move.
    pub new_owner_was: String,
    /// The registration the result keeps for the old owner.
    pub old_owner_registration: crate::model::DeviceRegistration,
    /// Each machine's Herdr socket, which names it in the ledger's host
    /// scopes while it owns the core. Both are required: a scope naming a
    /// machine whose socket is unknown would keep its old id unnoticed.
    pub old_owner_herdr_socket: String,
    pub new_owner_herdr_socket: String,
}

impl OwnerChange {
    fn device(&self, id: &str) -> Option<String> {
        if id == self.new_owner_was {
            Some(self.new_owner.clone())
        } else if id == self.old_owner {
            Some(self.old_owner_as.clone())
        } else {
            None
        }
    }

    /// A Herdr-scoped id (`pane`, `tab`): unqualified for the owner,
    /// `remote:<device>:<kind>:<raw>` for every other machine.
    fn scoped(&self, kind: &str, id: &str) -> Option<String> {
        let target = format!("remote:{}:{kind}:", self.new_owner_was);
        if let Some(raw) = id.strip_prefix(&target) {
            return Some(raw.to_owned());
        }
        if id.starts_with("remote:") || id.is_empty() {
            return None;
        }
        Some(format!("remote:{}:{kind}:{id}", self.old_owner_as))
    }
}

/// The registration and checkout ids of the two machines before and after
/// the move. The source core computes it from what it knows of each path
/// before it stops (`id_table`), because a project id is a digest of a root
/// only the machine holding it can resolve.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IdTable {
    pub registrations: BTreeMap<String, String>,
    pub checkouts: BTreeMap<String, String>,
}

/// One registered project of either machine, as the source core knows it.
pub struct KnownProject<'a> {
    pub id: &'a str,
    pub device_id: &'a str,
    pub path: &'a str,
    /// The repository root the project id digests; the old owner's are
    /// resolved on the source machine, the new owner's are its registered
    /// path, which its node reported as the root.
    pub root: &'a Path,
    pub checkouts: Vec<(&'a str, &'a str)>,
}

/// The ids each known project and checkout of the two machines takes, made
/// with the functions the core makes them with.
pub fn id_table<'a>(
    change: &OwnerChange,
    projects: impl IntoIterator<Item = KnownProject<'a>>,
) -> IdTable {
    let mut table = IdTable::default();
    for project in projects {
        if project.device_id == change.old_owner {
            let id = crate::device_catalog::project_id(&change.old_owner_as, project.root);
            table.registrations.insert(project.id.to_owned(), id);
            for (checkout, path) in project.checkouts {
                table.checkouts.insert(
                    checkout.to_owned(),
                    crate::device_catalog::checkout_id(&change.old_owner_as, path),
                );
            }
        } else if project.device_id == change.new_owner_was {
            let id = crate::workspace::workspace_id_for_path(Path::new(project.path));
            for (checkout, path) in project.checkouts {
                table.checkouts.insert(
                    checkout.to_owned(),
                    crate::workspace::checkout_id_for_path(&id, Path::new(path)),
                );
            }
            table.registrations.insert(project.id.to_owned(), id);
        }
    }
    table
}

/// What an owner change did, for the move's diagnostic record.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct ReownOutcome {
    pub files: Vec<String>,
    /// Fold and recent entries naming a checkout or project neither machine
    /// still has, dropped because no id can be made for them.
    pub pruned: usize,
    /// Why the link record could not be opened, when it could not: it moves
    /// as it is and the new core's link worker rebuilds it, as after layer 1.
    pub links_unopened: Option<String>,
}

/// Changes the owner of the copy at `staging` from `change.old_owner` to
/// `change.new_owner`. A refusal names the file and leaves the copy for the
/// caller to remove; the copy is never used unless this returns.
pub fn reown(staging: &Path, change: &OwnerChange, ids: &IdTable) -> Result<ReownOutcome, Refusal> {
    let refuse = |file: &Path, reason: String| Refusal {
        file: file.to_path_buf(),
        reason,
    };
    let marker_path = staging.join(MARKER_FILE);
    match read_marker(&marker_path).map_err(|reason| refuse(&marker_path, reason))? {
        Some(owner) if owner == change.old_owner => {}
        Some(owner) => {
            return Err(refuse(
                &marker_path,
                format!(
                    "this folder belongs to node {owner}, not to {}, the machine the core \
                     moves from",
                    change.old_owner
                ),
            ));
        }
        None => {
            return Err(refuse(
                &marker_path,
                "this folder has no owner yet; the layer-1 conversion runs first".to_owned(),
            ));
        }
    }
    NodeId::parse(&change.new_owner).map_err(|error| refuse(staging, error))?;

    let mut outcome = ReownOutcome::default();
    let mut mapper = Mapper {
        change,
        ids,
        pruned: 0,
    };
    for file in [
        CORE_STATE,
        WORKSPACE_VIEWS,
        LABELS,
        DELIVERY_LEDGER,
        LOCAL_ISSUES,
    ] {
        let path = staging.join(file);
        let Some(mut value) = read_json(&path).map_err(|reason| refuse(&path, reason))? else {
            continue;
        };
        let before = value.clone();
        for (_, pattern, _, moves) in KEYS.iter().filter(|(listed, ..)| *listed == file) {
            mapper
                .apply(&mut value, pattern, *moves)
                .map_err(|reason| refuse(&path, format!("{pattern}: {reason}")))?;
        }
        mapper
            .whole_file(file, &mut value)
            .map_err(|reason| refuse(&path, reason))?;
        if let Some(place) = still_naming(&value, change, "") {
            return Err(refuse(
                &path,
                format!(
                    "{place} still names {} after the move; no row of the key table covers it",
                    change.new_owner_was
                ),
            ));
        }
        if value != before {
            let bytes =
                serde_json::to_vec(&value).map_err(|error| refuse(&path, error.to_string()))?;
            write(&path, &bytes).map_err(|reason| refuse(&path, reason))?;
            outcome.files.push(file.to_owned());
        }
    }

    let links = staging.join(LINKS);
    if links.is_file() {
        match reown_links(&links, change).map_err(|reason| refuse(&links, reason))? {
            Links::Changed => outcome.files.push(LINKS.to_owned()),
            Links::Unchanged => {}
            Links::Unopened(reason) => outcome.links_unopened = Some(reason),
        }
    }

    let marker = serde_json::to_vec(&Marker {
        version: MARKER_VERSION,
        node: change.new_owner.clone(),
    })
    .map_err(|error| refuse(&marker_path, error.to_string()))?;
    write(&marker_path, &marker).map_err(|reason| refuse(&marker_path, reason))?;
    outcome.pruned = mapper.pruned;
    Ok(outcome)
}

struct Mapper<'a> {
    change: &'a OwnerChange,
    ids: &'a IdTable,
    pruned: usize,
}

/// What a mapping makes of one value: the same, another, or nothing (an
/// entry of a fold or recent list that names what neither machine has).
enum Mapped {
    Same,
    To(String),
    Drop,
}

impl Mapper<'_> {
    fn map(&self, moves: Moves, value: &str) -> Mapped {
        let to = |mapped: Option<String>| mapped.map_or(Mapped::Same, Mapped::To);
        match moves {
            Moves::Device => to(self.change.device(value)),
            Moves::Pane => to(self.change.scoped("pane", value)),
            Moves::Tab => to(self.change.scoped("tab", value)),
            Moves::Registration => self.table(&self.ids.registrations, value),
            Moves::Checkout => self.table(&self.ids.checkouts, value),
            Moves::Fold => {
                if let Some(device) = value.strip_prefix("cleanup/") {
                    to(self.change.device(device).map(|id| format!("cleanup/{id}")))
                } else {
                    self.table(&self.ids.registrations, value)
                }
            }
            Moves::Stays | Moves::OwnPath(_) | Moves::Labels | Moves::Whole => Mapped::Same,
        }
    }

    /// A registration or checkout id of either machine maps through the
    /// table; one of another machine stays; one of the two machines the table
    /// lacks names something neither still has.
    fn table(&self, table: &BTreeMap<String, String>, value: &str) -> Mapped {
        if let Some(id) = table.get(value) {
            return Mapped::To(id.clone());
        }
        let theirs = format!("remote:{}:", self.change.new_owner_was);
        if value.starts_with("workspace:") || value.starts_with(&theirs) {
            Mapped::Drop
        } else {
            Mapped::Same
        }
    }

    fn apply(&mut self, value: &mut Value, pattern: &str, moves: Moves) -> Result<(), String> {
        if matches!(
            moves,
            Moves::Stays | Moves::OwnPath(_) | Moves::Labels | Moves::Whole
        ) {
            return Ok(());
        }
        let parts: Vec<&str> = pattern.trim_start_matches('/').split('/').collect();
        let mut pruned = 0;
        let result = walk(value, &parts, &mut |slot| match slot {
            Slot::Value(value) => {
                let Some(text) = value.as_str() else {
                    return Ok(false);
                };
                match self.map(moves, text) {
                    Mapped::Same => Ok(false),
                    Mapped::To(id) => {
                        *value = Value::String(id);
                        Ok(false)
                    }
                    Mapped::Drop => {
                        pruned += 1;
                        Ok(true)
                    }
                }
            }
            Slot::Keys(map) => {
                let keys: Vec<String> = map.keys().cloned().collect();
                let mut renamed = Map::new();
                for key in keys {
                    let entry = map.remove(&key).expect("listed key");
                    let key = match self.map(moves, &key) {
                        Mapped::Same => key,
                        Mapped::To(id) => id,
                        Mapped::Drop => {
                            pruned += 1;
                            continue;
                        }
                    };
                    if renamed.insert(key.clone(), entry).is_some() {
                        return Err(format!("two entries would both be {key}"));
                    }
                }
                *map = renamed;
                Ok(false)
            }
        });
        self.pruned += pruned;
        result.map(|_| ())
    }

    /// The rows a pattern cannot express: the owner's path-keyed settings
    /// and the `device_*` maps beside them, the device registrations, the
    /// labels, the ledger's host scopes and spawn machines, the local issues
    /// and the View layouts' tabs.
    fn whole_file(&mut self, file: &str, value: &mut Value) -> Result<(), String> {
        match file {
            CORE_STATE => {
                for (own, devices) in OWN_PATHS {
                    swap_own(value, own, devices, self.change)?;
                }
                // A recent entry is its checkout: one whose checkout neither
                // machine still has goes, as a fold's does (its id was
                // counted as it was cleared).
                if let Some(recent) = value
                    .get_mut("recent_checkouts")
                    .and_then(Value::as_array_mut)
                {
                    recent.retain(|row| row.get("checkout_id").is_some_and(Value::is_string));
                }
                self.device_registrations(value)
            }
            WORKSPACE_VIEWS => {
                for view in each(value, "workspaces") {
                    if let Some(layout) = view.get_mut("agent_layout") {
                        self.agent_layout(layout);
                    }
                }
                Ok(())
            }
            LABELS => reown_labels(value, self.change),
            DELIVERY_LEDGER => {
                for agent in each(value, "agents") {
                    let scope = agent.get("host_scope").and_then(Value::as_str);
                    let mapped = match scope {
                        Some(scope) if scope == self.change.old_owner_herdr_socket => {
                            Some(Value::String(self.change.old_owner_as.clone()))
                        }
                        Some(scope) if scope == self.change.new_owner_was => {
                            Some(Value::String(self.change.new_owner_herdr_socket.clone()))
                        }
                        _ => None,
                    };
                    if let Some(mapped) = mapped {
                        agent["host_scope"] = mapped;
                    }
                }
                for spawn in each(value, "spawns") {
                    let Some(object) = spawn.as_object_mut() else {
                        continue;
                    };
                    match object.get("machine").and_then(Value::as_str) {
                        // A child on the owner's machine names none.
                        None => {
                            object.insert(
                                "machine".to_owned(),
                                Value::String(self.change.old_owner_as.clone()),
                            );
                        }
                        Some(machine) if machine == self.change.new_owner_was => {
                            object.remove("machine");
                        }
                        Some(_) => {}
                    }
                }
                Ok(())
            }
            LOCAL_ISSUES => swap_own(value, "projects", "devices", self.change),
            _ => Ok(()),
        }
    }

    /// The old owner gets the registration the change carries; the new
    /// owner's own registration goes, since a core does not register its own
    /// machine.
    fn device_registrations(&mut self, value: &mut Value) -> Result<(), String> {
        let Some(object) = value.as_object_mut() else {
            return Ok(());
        };
        let registrations = object
            .entry("device_registrations")
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(rows) = registrations.as_array_mut() else {
            return Err("device_registrations is not a list".to_owned());
        };
        // The old owner takes the new owner's place in the list, so a move
        // and a move back leave the devices in the order they were.
        let at = rows.iter().position(|row| {
            row.get("id").and_then(Value::as_str) == Some(&self.change.new_owner_was)
        });
        rows.retain(|row| {
            let id = row.get("id").and_then(Value::as_str);
            id != Some(self.change.new_owner_was.as_str())
                && id != Some(self.change.old_owner_as.as_str())
        });
        let registration = serde_json::to_value(&self.change.old_owner_registration)
            .map_err(|error| error.to_string())?;
        match at {
            Some(at) if at <= rows.len() => rows.insert(at, registration),
            _ => rows.push(registration),
        }
        Ok(())
    }

    /// Every tab an Agent layout names: each area's front display and its
    /// displays, and each canvas's tab.
    fn agent_layout(&mut self, layout: &mut Value) {
        fn node(mapper: &Mapper<'_>, value: &mut Value) {
            if let Some(area) = value.get_mut("area") {
                if let Some(active) = area.get_mut("active") {
                    tab(mapper, active);
                }
                for display in each(area, "displays") {
                    if let Some(id) = display.get_mut("id") {
                        tab(mapper, id);
                    }
                }
            }
            if let Some(split) = value.get_mut("split") {
                for side in ["first", "second"] {
                    if let Some(child) = split.get_mut(side) {
                        node(mapper, child);
                    }
                }
            }
        }
        fn tab(mapper: &Mapper<'_>, value: &mut Value) {
            if let Some(Mapped::To(id)) = value.as_str().map(|text| mapper.map(Moves::Tab, text)) {
                *value = Value::String(id);
            }
        }
        if let Some(root) = layout.get_mut("root") {
            node(self, root);
        }
        if let Some(canvases) = layout.get_mut("canvases").and_then(Value::as_object_mut) {
            for canvas in canvases.values_mut() {
                tab(self, canvas);
            }
        }
    }
}

/// The owner's path-keyed settings and the map beside each that keeps the
/// same setting for every other machine, keyed by its id (PRD
/// core-host-node-move Q19).
pub const OWN_PATHS: &[(&str, &str)] = &[
    ("expanded_paths", "device_expanded_paths"),
    ("project_base_branches", "device_project_base_branches"),
    ("project_issue_sources", "device_project_issue_sources"),
    (
        "expanded_inactive_checkout_project_paths",
        "device_expanded_inactive_checkout_project_paths",
    ),
    ("selected_path", "device_selected_paths"),
];

/// The old owner's `own` value moves under `devices[old_owner_as]`, and the
/// new owner's entry in `devices` becomes `own`; the rest of `devices`
/// changes only its keys. A new owner with no entry keeps `own` as an empty
/// value of its shape, because the reader requires some of these fields.
fn swap_own(
    value: &mut Value,
    own: &str,
    devices: &str,
    change: &OwnerChange,
) -> Result<(), String> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    let mine = object.remove(own);
    let emptied = mine.as_ref().map(|value| match value {
        Value::Array(_) => Value::Array(Vec::new()),
        Value::Object(_) => Value::Object(Map::new()),
        _ => Value::Null,
    });
    let mine = mine.filter(|value| !value.is_null());
    let mut map = match object.remove(devices) {
        Some(Value::Object(map)) => map,
        None | Some(Value::Null) => Map::new(),
        Some(_) => return Err(format!("{devices} is not a map")),
    };
    let theirs = map.remove(&change.new_owner_was);
    if map.contains_key(&change.old_owner_as) {
        return Err(format!(
            "{devices} already keeps {} as another machine",
            change.old_owner_as
        ));
    }
    if let Some(mine) = mine.filter(|value| !is_empty(value)) {
        map.insert(change.old_owner_as.clone(), mine);
    }
    if let Some(own_value) = theirs.or(emptied) {
        object.insert(own.to_owned(), own_value);
    }
    if !map.is_empty() {
        object.insert(devices.to_owned(), Value::Object(map));
    }
    Ok(())
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::Null => true,
        _ => false,
    }
}

/// `labels.json` keeps only the owner's records on disk; a device's stay in
/// memory, because a record holds the conversation it was made from
/// (`labels/store.rs`). The old owner's records therefore travel one hop as
/// `moved`, which the new core takes into memory for that machine and drops
/// from disk by the save its open makes at once; the new owner's, which the source core held
/// in memory and wrote to `moved` when it stopped, become its own.
fn reown_labels(value: &mut Value, change: &OwnerChange) -> Result<(), String> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    let mut targets = match object.remove("targets") {
        Some(Value::Object(map)) => map,
        None => Map::new(),
        Some(_) => return Err("targets is not a map".to_owned()),
    };
    let mut moved = match object.remove("moved") {
        Some(Value::Object(map)) => map,
        None => Map::new(),
        Some(_) => return Err("moved is not a map".to_owned()),
    };
    let mine = targets.remove(&change.old_owner);
    if !targets.is_empty() {
        return Err(format!(
            "targets keeps records of {}, which only the owner's may be",
            targets.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    let theirs = moved
        .remove(&crate::labels::device_target(&change.new_owner_was))
        .unwrap_or_default();
    let mut result = Map::new();
    if !is_empty(&theirs) {
        result.insert(change.new_owner.clone(), theirs);
    }
    object.insert("targets".to_owned(), Value::Object(result));
    // Only the old owner's travel on; any other machine's are read again
    // through its node, as after a restart.
    let mut next = Map::new();
    if let Some(mine) = mine.filter(|value| !is_empty(value)) {
        next.insert(crate::labels::device_target(&change.old_owner_as), mine);
    }
    if !next.is_empty() {
        object.insert("moved".to_owned(), Value::Object(next));
    }
    Ok(())
}

/// The link record's device column and its listing stamps: the new owner's
/// rows become the owner's, the old owner's keep or take its new id.
enum Links {
    Changed,
    Unchanged,
    Unopened(String),
}

fn reown_links(path: &Path, change: &OwnerChange) -> Result<Links, String> {
    let mut store = match crate::links::store::LinkStore::open(path) {
        Ok((store, _)) => store,
        Err(error) => return Ok(Links::Unopened(error.to_string())),
    };
    let mut changed = false;
    let listed = |device: &str| format!("{}:{device}", crate::links::worker::LISTED_AT);
    let own_listed = store.meta(crate::links::worker::LISTED_AT)?;
    let their_listed = store.meta(&listed(&change.new_owner_was))?;
    if change.old_owner_as != change.old_owner && store.has_device(&change.old_owner)? {
        store.convert_device(&change.old_owner, &change.old_owner_as)?;
        changed = true;
    }
    if change.new_owner_was != change.new_owner && store.has_device(&change.new_owner_was)? {
        store.convert_device(&change.new_owner_was, &change.new_owner)?;
        changed = true;
    }
    if let Some(stamp) = own_listed {
        store.set_meta(&listed(&change.old_owner_as), &stamp)?;
        store.delete_meta(crate::links::worker::LISTED_AT)?;
        changed = true;
    }
    if let Some(stamp) = their_listed {
        store.set_meta(crate::links::worker::LISTED_AT, &stamp)?;
        store.delete_meta(&listed(&change.new_owner_was))?;
        changed = true;
    }
    Ok(if changed {
        Links::Changed
    } else {
        Links::Unchanged
    })
}

/// What a pattern reaches: a value, or the object whose keys it names.
enum Slot<'a> {
    Value(&'a mut Value),
    Keys(&'a mut Map<String, Value>),
}

/// Calls `visit` for each slot `parts` reach (`*` is each array element or
/// object value, `{key}` the object's keys); a value slot answering `true`
/// is removed from the array or object holding it.
fn walk(
    value: &mut Value,
    parts: &[&str],
    visit: &mut dyn FnMut(Slot<'_>) -> Result<bool, String>,
) -> Result<bool, String> {
    let Some((part, rest)) = parts.split_first() else {
        return visit(Slot::Value(value));
    };
    match *part {
        "{key}" => {
            if let Value::Object(map) = value {
                if rest.is_empty() {
                    visit(Slot::Keys(map))?;
                } else {
                    for entry in map.values_mut() {
                        walk(entry, rest, visit)?;
                    }
                }
            }
            Ok(false)
        }
        "*" => {
            match value {
                Value::Array(items) => {
                    let mut kept = Vec::with_capacity(items.len());
                    for mut item in std::mem::take(items) {
                        if !walk(&mut item, rest, visit)? || !rest.is_empty() {
                            kept.push(item);
                        }
                    }
                    *items = kept;
                }
                Value::Object(map) => {
                    let keys: Vec<String> = map.keys().cloned().collect();
                    for key in keys {
                        let drop = walk(map.get_mut(&key).expect("listed key"), rest, visit)?;
                        if drop && rest.is_empty() {
                            map.remove(&key);
                        }
                    }
                }
                _ => {}
            }
            Ok(false)
        }
        key => {
            let Value::Object(map) = value else {
                return Ok(false);
            };
            let Some(entry) = map.get_mut(key) else {
                return Ok(false);
            };
            let drop = walk(entry, rest, visit)?;
            if drop && rest.is_empty() {
                // A single value naming nothing either machine has is cleared.
                map.insert(key.to_owned(), Value::Null);
            }
            Ok(false)
        }
    }
}

fn each<'a>(value: &'a mut Value, key: &str) -> impl Iterator<Item = &'a mut Value> {
    value
        .get_mut(key)
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
}

/// The fields whose value is a machine id.
const MACHINE_FIELDS: &[&str] = &[
    "device_id",
    "focused_device_id",
    "machine",
    "native_machine",
    "node_id",
    "target_id",
    "host_scope",
];

/// The first place, as a JSON pointer, where a key still names the new
/// owner by its old id: any value or object key carrying it as
/// `remote:<id>:`, and, when that id is an alias rather than its node id, a
/// machine field or an object key that is the alias itself (or its session
/// fold). Free text (a letter body, a label's conversation) is not a key and
/// is not read.
fn still_naming(value: &Value, change: &OwnerChange, at: &str) -> Option<String> {
    let old_id = change.new_owner_was.as_str();
    let qualified = format!("remote:{old_id}:");
    // A new owner known by its node id keeps that id as the owner's.
    let alias = old_id != change.new_owner;
    let carries = |text: &str| text.starts_with(&qualified);
    let is_alias = |text: &str| alias && (text == old_id || text == format!("cleanup/{old_id}"));
    match value {
        Value::String(text) if carries(text) => Some(at.to_owned()),
        Value::String(text)
            if is_alias(text)
                && at
                    .rsplit('/')
                    .next()
                    .is_some_and(|field| MACHINE_FIELDS.contains(&field)) =>
        {
            Some(at.to_owned())
        }
        Value::Array(items) => items.iter().enumerate().find_map(|(index, item)| {
            let place = format!("{at}/{index}");
            match item.as_str() {
                // A list of machine ids or session folds.
                Some(text) if is_alias(text) => Some(place),
                _ => still_naming(item, change, &place),
            }
        }),
        Value::Object(map) => map.iter().find_map(|(key, entry)| {
            let place = format!("{at}/{key}");
            if carries(key) || is_alias(key) {
                return Some(place);
            }
            if matches!(key.as_str(), "body" | "facts") {
                return None;
            }
            still_naming(entry, change, &place)
        }),
        _ => None,
    }
}

/// Where a staging copy lives beside the state folder it was made from.
pub fn staging_dir(state_dir: &Path, intent: &str) -> PathBuf {
    state_dir.join(super::MOVE_STAGING).join(intent)
}

#[cfg(test)]
mod tests {
    use super::super::tests::reach;
    use super::*;
    use serde_json::json;

    /// The MacBook, whose core wrote the folder, and the Mac mini, which the
    /// folder knows as the dialed device `mini`, and a third device that
    /// neither move touches.
    const M: &str = "8f1c2d3e-4a5b-4c6d-8e7f-90a1b2c3d4e5";
    const C: &str = "2b7e9f10-1111-4a2b-9c3d-5e6f7a8b9c0d";
    const ALIAS: &str = "mini";
    const X: &str = "studio";
    const M_ROOT: &str = "/Users/op/alpha";
    const M_WORKTREE: &str = "/Users/op/alpha-review";
    const C_ROOT: &str = "/Users/grab/beta";
    const X_ROOT: &str = "/srv/gamma";
    const M_SOCKET: &str = "/Users/op/.config/herdr/herdr.sock";
    const C_SOCKET: &str = "/Users/grab/.config/herdr/herdr.sock";

    fn mini_registration() -> Value {
        json!({"id": ALIAS, "label": "Mac mini", "ssh_alias": "mini", "herdr_socket_path": C_SOCKET})
    }

    fn forward() -> OwnerChange {
        OwnerChange {
            old_owner: M.into(),
            old_owner_as: M.into(),
            new_owner: C.into(),
            new_owner_was: ALIAS.into(),
            old_owner_registration: serde_json::from_value(
                json!({"id": M, "label": "MacBook", "inbound": true}),
            )
            .unwrap(),
            old_owner_herdr_socket: M_SOCKET.into(),
            new_owner_herdr_socket: C_SOCKET.into(),
        }
    }

    fn back() -> OwnerChange {
        OwnerChange {
            old_owner: C.into(),
            old_owner_as: ALIAS.into(),
            new_owner: M.into(),
            new_owner_was: M.into(),
            old_owner_registration: serde_json::from_value(mini_registration()).unwrap(),
            old_owner_herdr_socket: C_SOCKET.into(),
            new_owner_herdr_socket: M_SOCKET.into(),
        }
    }

    fn own_workspace() -> String {
        crate::workspace::workspace_id_for_path(Path::new(M_ROOT))
    }
    fn own_checkout(path: &str) -> String {
        crate::workspace::checkout_id_for_path(&own_workspace(), Path::new(path))
    }
    fn device_project(device: &str, root: &str) -> String {
        crate::device_catalog::project_id(device, Path::new(root))
    }
    fn device_checkout(device: &str, path: &str) -> String {
        crate::device_catalog::checkout_id(device, path)
    }

    /// The table the MacBook's core makes before it stops.
    fn forward_ids() -> IdTable {
        let own = own_workspace();
        let mini = device_project(ALIAS, C_ROOT);
        let main = own_checkout(M_ROOT);
        let review = own_checkout(M_WORKTREE);
        let theirs = device_checkout(ALIAS, C_ROOT);
        id_table(
            &forward(),
            [
                KnownProject {
                    id: &own,
                    device_id: M,
                    path: M_ROOT,
                    root: Path::new(M_ROOT),
                    checkouts: vec![(&main, M_ROOT), (&review, M_WORKTREE)],
                },
                KnownProject {
                    id: &mini,
                    device_id: ALIAS,
                    path: C_ROOT,
                    root: Path::new(C_ROOT),
                    checkouts: vec![(&theirs, C_ROOT)],
                },
            ],
        )
    }

    /// The table the mini's core makes before it stops for a move back.
    fn back_ids() -> IdTable {
        let own = crate::workspace::workspace_id_for_path(Path::new(C_ROOT));
        let main = crate::workspace::checkout_id_for_path(&own, Path::new(C_ROOT));
        let mac = device_project(M, M_ROOT);
        let mac_main = device_checkout(M, M_ROOT);
        let mac_review = device_checkout(M, M_WORKTREE);
        id_table(
            &back(),
            [
                KnownProject {
                    id: &own,
                    device_id: C,
                    path: C_ROOT,
                    root: Path::new(C_ROOT),
                    checkouts: vec![(&main, C_ROOT)],
                },
                KnownProject {
                    id: &mac,
                    device_id: M,
                    path: M_ROOT,
                    root: Path::new(M_ROOT),
                    checkouts: vec![(&mac_main, M_ROOT), (&mac_review, M_WORKTREE)],
                },
            ],
        )
    }

    /// One value of each shape for a kind of id: the owner's, the dialed
    /// mini's and the third device's, and what each is after the move.
    fn shapes(kind: &str) -> Vec<(String, String)> {
        let pane = |raw: &str| raw.to_owned();
        match kind {
            "pane" | "tab" => vec![
                (
                    pane("w1:x1").replace('x', &kind[..1]),
                    format!("remote:{M}:{kind}:w1:{}1", &kind[..1]),
                ),
                (
                    format!("remote:{ALIAS}:{kind}:w2:{}1", &kind[..1]),
                    format!("w2:{}1", &kind[..1]),
                ),
                (
                    format!("remote:{X}:{kind}:w3:{}1", &kind[..1]),
                    format!("remote:{X}:{kind}:w3:{}1", &kind[..1]),
                ),
            ],
            "device" => vec![
                (M.into(), M.into()),
                (ALIAS.into(), C.into()),
                (X.into(), X.into()),
            ],
            "registration" => vec![
                (own_workspace(), device_project(M, M_ROOT)),
                (
                    device_project(ALIAS, C_ROOT),
                    crate::workspace::workspace_id_for_path(Path::new(C_ROOT)),
                ),
                (device_project(X, X_ROOT), device_project(X, X_ROOT)),
            ],
            "checkout" => vec![
                (own_checkout(M_WORKTREE), device_checkout(M, M_WORKTREE)),
                (
                    device_checkout(ALIAS, C_ROOT),
                    crate::workspace::checkout_id_for_path(
                        &crate::workspace::workspace_id_for_path(Path::new(C_ROOT)),
                        Path::new(C_ROOT),
                    ),
                ),
                (device_checkout(X, X_ROOT), device_checkout(X, X_ROOT)),
            ],
            "fold" => vec![
                (own_workspace(), device_project(M, M_ROOT)),
                (format!("cleanup/{ALIAS}"), format!("cleanup/{C}")),
                (format!("cleanup/{X}"), format!("cleanup/{X}")),
            ],
            other => panic!("no shapes for {other}"),
        }
    }

    fn before(kind: &str) -> Vec<String> {
        shapes(kind).into_iter().map(|(before, _)| before).collect()
    }

    fn keyed(kind: &str, value: Value) -> Value {
        Value::Object(
            before(kind)
                .into_iter()
                .map(|key| (key, value.clone()))
                .collect(),
        )
    }

    /// A folder as the MacBook's core leaves it at a move's stop: a value of
    /// every shape at every key the table names, and the labels the core
    /// held in memory for the mini written to `moved`.
    fn macbook_folder() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path();
        let write = |name: &str, value: Value| {
            std::fs::write(state.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
        };
        let panes = before("pane");
        let devices = before("device");
        let checkouts = before("checkout");
        let registrations = before("registration");
        let actor = |index: usize| json!({"pane_id": panes[index], "name": "a", "kind": "claude", "device_id": devices[index]});
        write(
            CORE_STATE,
            json!({
                "schema_version": 1,
                "workspace_registrations": [
                    {"id": registrations[0], "label": "alpha", "path": M_ROOT, "device_id": M, "primary_checkout_id": checkouts[0]},
                    {"id": registrations[1], "label": "beta", "path": C_ROOT, "device_id": ALIAS, "primary_checkout_id": checkouts[1]},
                    {"id": registrations[2], "label": "gamma", "path": X_ROOT, "device_id": X, "primary_checkout_id": checkouts[2]},
                ],
                "device_registrations": [mini_registration(), {"id": X, "label": "Studio", "ssh_alias": "studio"}],
                "focused_device_id": ALIAS,
                "expanded_inactive_project_device_ids": devices,
                "recent_checkouts": (0..3).map(|index| json!({
                    "device_id": devices[index], "checkout_id": checkouts[index],
                    "project_name": "p", "branch": "main", "device_name": "d",
                })).collect::<Vec<_>>(),
                "expanded_paths": [M_ROOT],
                "device_expanded_paths": {ALIAS: [C_ROOT], X: [X_ROOT]},
                "selected_path": M_ROOT,
                "device_selected_paths": {ALIAS: C_ROOT, X: X_ROOT},
                "project_base_branches": {M_ROOT: "main"},
                "device_project_base_branches": {ALIAS: {C_ROOT: "trunk"}, X: {X_ROOT: "dev"}},
                "project_issue_sources": {M_ROOT: "github"},
                "device_project_issue_sources": {ALIAS: {C_ROOT: "local"}, X: {X_ROOT: "github"}},
                "expanded_inactive_checkout_project_paths": [M_ROOT],
                "device_expanded_inactive_checkout_project_paths": {ALIAS: [C_ROOT], X: [X_ROOT]},
                "sessions_mode_by_project": {hide_project::project_id(M, Path::new(M_ROOT)): "memory"},
                "selected_pane_id": panes[1],
                "expanded_agent_pane_ids": panes,
                "sessions_expanded_agent_pane_ids": panes,
                "recent_pane_ids": panes,
                "pane_text_scales": keyed("pane", json!(1.2)),
                "pane_read_records": Value::Object(panes.iter().enumerate().map(|(index, pane)| (
                    pane.clone(),
                    json!({"demand": "none", "activity": "idle", "descendant_signals": [{"pane_id": panes[index]}]}),
                )).collect()),
                "request_verbs": keyed("pane", json!({"verb": "review"})),
                "pane_terminal_sizes": keyed("pane", json!([80, 24])),
                "resolved_sessions": keyed("pane", json!({"at_unix_ms": 1})),
                "session_resolution_inputs": keyed("pane", json!(1)),
                "agent_sleep": {
                    "stamps": keyed("pane", json!({})),
                    "records": Value::Object(panes.iter().enumerate().map(|(index, pane)| (
                        pane.clone(),
                        json!({"parent_pane_id": panes[index], "cwd": M_ROOT}),
                    )).collect()),
                    "dormant": {"sleep-1": {"node_id": M, "old_pane_id": panes[0], "cwd": M_ROOT}},
                },
                "factory_secretary_pane": panes[0],
                "focused_checkout_id": checkouts[1],
                "collapsed_workspace_ids": registrations,
                "collapsed_checkout_ids": checkouts,
                "expanded_checkout_ids": checkouts,
                "session_collapsed_checkout_ids": checkouts,
                "session_open_folds": before("fold"),
            }),
        );
        let tabs = before("tab");
        let layout = |tab: &str| {
            json!({
                "root": {"split": {"id": "s1", "axis": "horizontal", "ratio": 0.5,
                    "first": {"area": {"id": "a1", "active": tab, "displays": [{"id": tab}]}},
                    "second": {"area": {"id": "a2", "active": null, "displays": []}}}},
                "active_area": "a1", "next_id": 3, "canvases": {"a2": tab},
            })
        };
        write(
            WORKSPACE_VIEWS,
            json!({"schema_version": 2, "workspaces": [
                {"device_id": M, "path": M_ROOT, "agent_layout": layout(&tabs[0]),
                 "view_bookmarks": {tabs[0].clone(): {"a1": "d1"}}, "layout": {"root": {"area": {"id": "a1"}}}},
                {"device_id": ALIAS, "path": C_ROOT, "agent_layout": layout(&tabs[1]),
                 "view_bookmarks": {tabs[1].clone(): {"a1": "d2"}}, "layout": {}},
                {"device_id": X, "path": X_ROOT, "agent_layout": layout(&tabs[2]),
                 "view_bookmarks": {tabs[2].clone(): {"a1": "d3"}}, "layout": {}},
            ]}),
        );
        write(
            LABELS,
            json!({
                "version": 1,
                "targets": {M: {"w1:p1": {"owner": "mac"}}},
                "moved": {"device:mini": {"w2:p1": {"owner": "mini"}}},
            }),
        );
        write(
            DELIVERY_LEDGER,
            json!({
                "version": 1, "next_id": 9,
                "letters": (0..3).map(|index| json!({
                    "sender": actor(index), "recipient": actor((index + 1) % 3),
                    "watch_warning": {"target": actor(index)}, "body": "from mini to studio",
                })).collect::<Vec<_>>(),
                "watches": (0..3).map(|index| json!({"parent": actor(index), "target": actor((index + 2) % 3)})).collect::<Vec<_>>(),
                "agents": [
                    {"machine": M, "native_machine": M, "actor": actor(0), "pane": "w1:p1", "host_scope": M_SOCKET, "project": M_ROOT, "session": "s1", "instance": "t1"},
                    {"machine": ALIAS, "native_machine": C, "actor": actor(1), "pane": "w2:p1", "host_scope": ALIAS, "project": C_ROOT, "session": "s2", "instance": "t2"},
                    {"machine": X, "native_machine": "studio-machine", "actor": actor(2), "pane": "w3:p1", "host_scope": X, "project": X_ROOT, "session": "s3", "instance": "t3"},
                ],
                "spawns": [
                    {"repo": M_ROOT, "path": M_ROOT, "requested_path": M_ROOT, "pane": "w1:p2"},
                    {"repo": C_ROOT, "path": C_ROOT, "requested_path": C_ROOT, "pane": "w2:p2", "machine": ALIAS},
                    {"repo": X_ROOT, "path": X_ROOT, "requested_path": X_ROOT, "pane": "w3:p2", "machine": X},
                ],
            }),
        );
        write(
            LOCAL_ISSUES,
            json!({"version": 1, "projects": {M_ROOT: [{"number": 1}]}, "devices": {ALIAS: {C_ROOT: [{"number": 2}]}}}),
        );
        write(MARKER_FILE, json!({"version": 1, "node": M}));
        let (links, _) = crate::links::store::LinkStore::open(&state.join(LINKS)).unwrap();
        links
            .set_meta(crate::links::worker::LISTED_AT, "100")
            .unwrap();
        links
            .set_meta(&format!("listed_at:{ALIAS}"), "200")
            .unwrap();
        drop(links);
        let connection = rusqlite::Connection::open(state.join(LINKS)).unwrap();
        for device in [M, ALIAS, X] {
            connection
                .execute(
                    "INSERT INTO sessions(device,agent,id) VALUES(?1,'claude','s')",
                    [device],
                )
                .unwrap();
        }
        dir
    }

    fn read(path: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn rows(state: &Path, file: &str) -> Vec<(&'static str, Moves)> {
        let _ = state;
        KEYS.iter()
            .filter(|(listed, ..)| *listed == file)
            .map(|(_, pattern, _, moves)| (*pattern, *moves))
            .collect()
    }

    fn kind(moves: Moves) -> Option<&'static str> {
        match moves {
            Moves::Device => Some("device"),
            Moves::Pane => Some("pane"),
            Moves::Tab => Some("tab"),
            Moves::Registration => Some("registration"),
            Moves::Checkout => Some("checkout"),
            Moves::Fold => Some("fold"),
            _ => None,
        }
    }

    /// A store compared as the core reads it: a null or empty value is the
    /// same as none.
    fn normalized(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .filter(|(_, entry)| !is_empty(entry))
                    .map(|(key, entry)| (key.clone(), normalized(entry)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(normalized).collect()),
            other => other.clone(),
        }
    }

    #[test]
    fn every_key_of_every_shape_is_spelled_as_the_new_core_writes_it() {
        let dir = macbook_folder();
        let state = dir.path();
        let original: BTreeMap<&str, Value> = [CORE_STATE, WORKSPACE_VIEWS, DELIVERY_LEDGER]
            .into_iter()
            .map(|file| (file, read(&state.join(file))))
            .collect();
        reown(state, &forward(), &forward_ids()).unwrap();

        for (file, value) in &original {
            let after = read(&state.join(file));
            for (pattern, moves) in rows(state, file) {
                let Some(kind) = kind(moves) else { continue };
                let reached = reach(value, pattern);
                assert!(
                    !reached.is_empty(),
                    "the fixture has no value at {file}{pattern}"
                );
                let mapping: BTreeMap<String, String> = shapes(kind).into_iter().collect();
                let mut expected: Vec<Value> = reached
                    .iter()
                    .map(|value| match value.as_str() {
                        Some(text) => json!(
                            mapping
                                .get(text)
                                .cloned()
                                .unwrap_or_else(|| text.to_owned())
                        ),
                        None => value.clone(),
                    })
                    .collect();
                let mut found = reach(&after, pattern);
                let key = |value: &Value| value.to_string();
                expected.sort_by_key(key);
                found.sort_by_key(key);
                assert_eq!(found, expected, "{file}{pattern}");
            }
        }

        let core = read(&state.join(CORE_STATE));
        assert_eq!(core["expanded_paths"], json!([C_ROOT]));
        assert_eq!(
            core["device_expanded_paths"],
            json!({M: [M_ROOT], X: [X_ROOT]})
        );
        assert_eq!(core["selected_path"], json!(C_ROOT));
        assert_eq!(core["device_selected_paths"], json!({M: M_ROOT, X: X_ROOT}));
        assert_eq!(core["project_base_branches"], json!({C_ROOT: "trunk"}));
        assert_eq!(
            core["device_project_base_branches"],
            json!({M: {M_ROOT: "main"}, X: {X_ROOT: "dev"}})
        );
        assert_eq!(core["project_issue_sources"], json!({C_ROOT: "local"}));
        // As the core reads them: the stored form spells absent fields as
        // null where the runtime writes them.
        let registrations = |value: Value| -> Vec<crate::model::DeviceRegistration> {
            serde_json::from_value(value).unwrap()
        };
        assert_eq!(
            registrations(core["device_registrations"].clone()),
            registrations(
                json!([{"id": M, "label": "MacBook", "inbound": true}, {"id": X, "label": "Studio", "ssh_alias": "studio"}])
            )
        );
        let views = read(&state.join(WORKSPACE_VIEWS));
        let mac = &views["workspaces"][0]["agent_layout"];
        let tab = format!("remote:{M}:tab:w1:t1");
        assert_eq!(mac["root"]["split"]["first"]["area"]["active"], json!(tab));
        assert_eq!(
            mac["root"]["split"]["first"]["area"]["displays"][0]["id"],
            json!(tab)
        );
        assert_eq!(mac["canvases"]["a2"], json!(tab));
        assert_eq!(
            views["workspaces"][1]["agent_layout"]["canvases"]["a2"],
            json!("w2:t1")
        );
        let ledger = read(&state.join(DELIVERY_LEDGER));
        assert_eq!(ledger["agents"][0]["host_scope"], json!(M));
        assert_eq!(ledger["agents"][1]["host_scope"], json!(C_SOCKET));
        assert_eq!(ledger["agents"][2]["host_scope"], json!(X));
        assert_eq!(ledger["spawns"][0]["machine"], json!(M));
        assert!(ledger["spawns"][1].get("machine").is_none());
        assert_eq!(ledger["spawns"][2]["machine"], json!(X));
        assert_eq!(ledger["letters"][0]["body"], json!("from mini to studio"));
        let labels = read(&state.join(LABELS));
        assert_eq!(labels["targets"], json!({C: {"w2:p1": {"owner": "mini"}}}));
        assert_eq!(
            labels["moved"],
            json!({format!("device:{M}"): {"w1:p1": {"owner": "mac"}}})
        );
        let issues = read(&state.join(LOCAL_ISSUES));
        assert_eq!(issues["projects"], json!({C_ROOT: [{"number": 2}]}));
        assert_eq!(issues["devices"], json!({M: {M_ROOT: [{"number": 1}]}}));
        let links = crate::links::store::LinkStore::open(&state.join(LINKS))
            .unwrap()
            .0;
        assert_eq!(links.meta("listed_at").unwrap().as_deref(), Some("200"));
        assert_eq!(
            links.meta(&format!("listed_at:{M}")).unwrap().as_deref(),
            Some("100")
        );
        assert!(!links.has_device(ALIAS).unwrap());
        assert!(
            links.has_device(C).unwrap()
                && links.has_device(M).unwrap()
                && links.has_device(X).unwrap()
        );
        assert_eq!(read(&state.join(MARKER_FILE))["node"], json!(C));
    }

    #[test]
    fn a_move_back_returns_every_store_to_what_it_was() {
        let dir = macbook_folder();
        let state = dir.path();
        let files = [
            CORE_STATE,
            WORKSPACE_VIEWS,
            LABELS,
            DELIVERY_LEDGER,
            LOCAL_ISSUES,
            MARKER_FILE,
        ];
        let original: Vec<Value> = files.iter().map(|file| read(&state.join(file))).collect();
        reown(state, &forward(), &forward_ids()).unwrap();
        // The mini's core takes the MacBook's records into memory and, when it
        // stops for the move back, writes them to `moved` again.
        let mut labels = read(&state.join(LABELS));
        let mac = labels["moved"]
            .as_object_mut()
            .unwrap()
            .remove(&format!("device:{M}"))
            .unwrap();
        labels["moved"] = json!({format!("device:{M}"): mac});
        std::fs::write(state.join(LABELS), serde_json::to_vec(&labels).unwrap()).unwrap();

        reown(state, &back(), &back_ids()).unwrap();
        for (file, original) in files.iter().zip(&original) {
            assert_eq!(
                normalized(&read(&state.join(file))),
                normalized(original),
                "{file}"
            );
        }
        let links = crate::links::store::LinkStore::open(&state.join(LINKS))
            .unwrap()
            .0;
        assert_eq!(links.meta("listed_at").unwrap().as_deref(), Some("100"));
        assert_eq!(
            links
                .meta(&format!("listed_at:{ALIAS}"))
                .unwrap()
                .as_deref(),
            Some("200")
        );
        assert!(!links.has_device(C).unwrap());
        assert!(links.has_device(ALIAS).unwrap() && links.has_device(M).unwrap());
    }

    #[test]
    fn the_core_state_a_move_writes_is_one_the_core_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CORE_STATE);
        crate::persistence::save(
            &path,
            &crate::model::UiStateSnapshot::default(),
            &Default::default(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join(MARKER_FILE),
            serde_json::to_vec(&json!({"version": 1, "node": M})).unwrap(),
        )
        .unwrap();
        reown(dir.path(), &forward(), &IdTable::default()).unwrap();
        let (_, _, disposition) = crate::persistence::load(&path);
        assert_eq!(disposition, crate::persistence::LoadDisposition::Loaded);
    }

    #[test]
    fn a_key_the_table_does_not_cover_stops_the_move_and_names_its_place() {
        let dir = macbook_folder();
        let state = dir.path();
        let mut core = read(&state.join(CORE_STATE));
        core["new_feature_panes"] = json!({"focus": format!("remote:{ALIAS}:pane:w2:p9")});
        std::fs::write(state.join(CORE_STATE), serde_json::to_vec(&core).unwrap()).unwrap();
        let refusal = reown(state, &forward(), &forward_ids()).unwrap_err();
        assert_eq!(refusal.file, state.join(CORE_STATE));
        assert!(
            refusal.reason.contains("/new_feature_panes/focus"),
            "{refusal}"
        );
        assert_eq!(
            read(&state.join(MARKER_FILE))["node"],
            json!(M),
            "the owner is unchanged"
        );
    }

    #[test]
    fn a_folder_another_node_owns_or_no_node_owns_yet_is_refused() {
        let dir = macbook_folder();
        let state = dir.path();
        std::fs::write(
            state.join(MARKER_FILE),
            br#"{"version":1,"node":"someone-else"}"#,
        )
        .unwrap();
        let core = std::fs::read(state.join(CORE_STATE)).unwrap();
        let refusal = reown(state, &forward(), &forward_ids()).unwrap_err();
        assert!(refusal.reason.contains("someone-else"), "{refusal}");
        assert_eq!(std::fs::read(state.join(CORE_STATE)).unwrap(), core);
        std::fs::remove_file(state.join(MARKER_FILE)).unwrap();
        let refusal = reown(state, &forward(), &forward_ids()).unwrap_err();
        assert!(refusal.reason.contains("no owner yet"), "{refusal}");
        assert_eq!(std::fs::read(state.join(CORE_STATE)).unwrap(), core);
    }

    #[test]
    fn fold_and_recent_entries_naming_what_neither_machine_has_are_dropped_and_counted() {
        let dir = macbook_folder();
        let state = dir.path();
        let mut core = read(&state.join(CORE_STATE));
        let gone =
            crate::workspace::checkout_id_for_path(&own_workspace(), Path::new("/Users/op/gone"));
        core["collapsed_checkout_ids"]
            .as_array_mut()
            .unwrap()
            .push(json!(gone));
        core["focused_checkout_id"] = json!(gone);
        let recent = core["recent_checkouts"].as_array().unwrap().len();
        core["recent_checkouts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"device_id": M, "checkout_id": gone, "project_name": "gone", "branch": "main", "device_name": "MacBook"}));
        std::fs::write(state.join(CORE_STATE), serde_json::to_vec(&core).unwrap()).unwrap();
        let outcome = reown(state, &forward(), &forward_ids()).unwrap();
        assert_eq!(outcome.pruned, 3);
        let core = read(&state.join(CORE_STATE));
        assert!(!core["collapsed_checkout_ids"].to_string().contains(&gone));
        assert_eq!(core["focused_checkout_id"], Value::Null);
        assert_eq!(core["recent_checkouts"].as_array().unwrap().len(), recent);
    }

    #[test]
    fn the_labels_a_move_carries_reach_the_new_core_in_memory_and_never_its_disk() {
        let dir = macbook_folder();
        let state = dir.path();
        reown(state, &forward(), &forward_ids()).unwrap();
        let store = crate::labels::store::LabelStore::open(Some(state), None, C);
        assert_eq!(
            store.target(C).len(),
            1,
            "the mini's records are the core's own"
        );
        let mac = crate::labels::device_target(M);
        assert_eq!(
            store.target(&mac).len(),
            1,
            "the MacBook's records are in memory"
        );
        store.flush();
        drop(store);
        let on_disk = read(&state.join(LABELS));
        assert!(on_disk.get("moved").is_none(), "{on_disk}");
        assert_eq!(
            on_disk["targets"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec![C]
        );
    }
}
