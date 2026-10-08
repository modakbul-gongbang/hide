//! Where a node's diagnostic records go. The process that owns the node
//! installs its log once (`hided` passes the core's), so a record the SSH
//! transport writes lands in the same Logs file as the core's; before that,
//! and in a process that installs none, a record goes to stderr.

use std::sync::OnceLock;

static SINK: OnceLock<fn(serde_json::Value)> = OnceLock::new();

/// Emits one JSON record.
#[macro_export]
macro_rules! diagnostic {
    ($value:expr) => {
        $crate::diagnostics::emit($value)
    };
}

/// Sends every later record to `sink`. The first install holds for the life
/// of the process; a second one is refused and answers false.
pub fn install(sink: fn(serde_json::Value)) -> bool {
    SINK.set(sink).is_ok()
}

pub fn emit(record: serde_json::Value) {
    match SINK.get() {
        Some(sink) => sink(record),
        None => eprintln!("{record}"),
    }
}
