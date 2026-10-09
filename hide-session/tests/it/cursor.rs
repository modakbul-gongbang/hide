//! The binary fixture was encoded by Cursor 2026.10.01's generated agent/v1
//! classes, independently of the reader. The agreed transcript is one request,
//! one answer and an invisible tool, with unknown/minute timing respectively.

use std::path::PathBuf;

use hide_session::{EventKind, RescanReason, cursor};
use rusqlite::Connection;
use serde_json::{Value, json};

const ID: &str = "a1b2c3d4-0000-4000-8000-000000000001";
const CREATED: u64 = 1_790_989_200_000;

struct Fixture {
    home: tempfile::TempDir,
    cwd: PathBuf,
    path: PathBuf,
    graph: Value,
}

fn bytes(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let cwd = home.path().join("project");
        std::fs::create_dir(&cwd).unwrap();
        let bucket = format!("{:x}", md5::compute(cwd.to_str().unwrap().as_bytes()));
        let path = home
            .path()
            .join(cursor::SESSIONS)
            .join(bucket)
            .join(ID)
            .join("store.db");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let graph: Value =
            serde_json::from_str(include_str!("../fixtures/cursor-2026.10.01/graph.json")).unwrap();
        let database = Connection::open(&path).unwrap();
        database.execute_batch("PRAGMA user_version=1; CREATE TABLE blobs(id TEXT PRIMARY KEY, data BLOB); CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);").unwrap();
        for (id, encoded) in graph["blobs"].as_object().unwrap() {
            database
                .execute(
                    "INSERT INTO blobs VALUES (?1,?2)",
                    rusqlite::params![id, bytes(encoded.as_str().unwrap())],
                )
                .unwrap();
        }
        drop(database);
        let fixture = Self {
            home,
            cwd,
            path,
            graph,
        };
        fixture.root("first");
        fixture.sidecar(json!({"schemaVersion":1,"createdAtMs":CREATED,"hasConversation":true,"cwd":fixture.cwd,"title":"Do not trust this title"}));
        fixture
    }

    fn root(&self, variant: &str) {
        let metadata = json!({"agentId":ID,"latestRootBlobId":self.graph["roots"][variant],"createdAt":CREATED,"name":"Do not trust this title","blobEncryptionKey":"secret-test-value"});
        let encoded: String = metadata
            .to_string()
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Connection::open(&self.path)
            .unwrap()
            .execute("INSERT OR REPLACE INTO meta VALUES ('0',?1)", [&encoded])
            .unwrap();
    }

    fn sidecar(&self, value: Value) {
        std::fs::write(self.path.with_file_name("meta.json"), value.to_string()).unwrap();
    }

    fn read(&self, saved: Option<cursor::Checkpoint>) -> hide_session::Result<cursor::ReadResult> {
        cursor::read(
            self.home.path(),
            &self.path,
            ID,
            self.cwd.to_str().unwrap(),
            saved,
            hide_session::SESSION_INCREMENT_READ_LIMIT_BYTES,
        )
    }
}

#[test]
fn native_units_title_negative_facts_and_time_precision() {
    let fixture = Fixture::new();
    let before = std::fs::read(&fixture.path).unwrap();
    let result = fixture.read(None).unwrap();
    assert_eq!(
        result
            .parsed
            .events
            .iter()
            .map(|event| (event.role, event.text.as_str(), event.at_unix_ms))
            .collect::<Vec<_>>(),
        vec![
            ("user", "Read this conversation", 0),
            ("assistant", "Native answer", 1_790_989_260_000)
        ]
    );
    assert_eq!(result.parsed.event_offsets, [0, 1]);
    assert!(result.parsed.title.is_none());
    assert!(result.parsed.custom_title.is_none());
    assert!(result.parsed.turn_marks.is_empty());
    assert!(result.parsed.pr_sightings.is_empty());
    assert!(!result.has_more);
    assert_eq!(std::fs::read(&fixture.path).unwrap(), before);
    assert!(!fixture.path.with_file_name("store.db-journal").exists());
    assert_eq!(
        cursor::header(fixture.home.path(), &fixture.path)
            .unwrap()
            .cwd,
        fixture.cwd
    );
}

#[test]
fn checkpoint_reads_mutated_last_turn_without_repeating_request() {
    let fixture = Fixture::new();
    let first = fixture.read(None).unwrap();
    fixture.root("append");
    let appended = fixture.read(Some(first.checkpoint)).unwrap();
    assert_eq!(appended.parsed.events.len(), 1);
    assert_eq!(appended.parsed.events[0].text, "Later answer");
    assert_eq!(appended.parsed.event_offsets, [3]);
    assert!(appended.parsed.rescan_reason.is_none());
    let idle = fixture.read(Some(appended.checkpoint)).unwrap();
    assert!(idle.parsed.events.is_empty());
}

#[test]
fn checkpoint_refuses_to_append_a_rewritten_prefix() {
    let fixture = Fixture::new();
    let first = fixture.read(None).unwrap();
    fixture.root("rewrite");
    let rewritten = fixture.read(Some(first.checkpoint)).unwrap();
    assert_eq!(
        rewritten.parsed.rescan_reason,
        Some(RescanReason::Truncated)
    );
    assert_eq!(
        rewritten
            .parsed
            .events
            .iter()
            .map(|event| event.text.as_str())
            .collect::<Vec<_>>(),
        ["Read this conversation", "Later answer"]
    );
}

#[test]
fn root_child_flags_and_checkout_mismatch_fail_locally() {
    let fixture = Fixture::new();
    fixture.sidecar(json!({"schemaVersion":1,"createdAtMs":CREATED,"hasConversation":true,"cwd":fixture.cwd,"isSubagent":true}));
    assert!(fixture.read(None).is_err());
    fixture.sidecar(json!({"schemaVersion":1,"createdAtMs":CREATED,"hasConversation":true,"cwd":fixture.home.path()}));
    assert!(fixture.read(None).is_err());
    fixture.sidecar(json!({"schemaVersion":1,"createdAtMs":CREATED,"hasConversation":true}));
    assert!(fixture.read(None).is_err());
}

#[test]
fn missing_tampered_and_oversized_blobs_fail_before_publishing() {
    for mode in ["missing", "tampered", "oversized"] {
        let fixture = Fixture::new();
        let db = Connection::open(&fixture.path).unwrap();
        let root = fixture.graph["roots"]["first"].as_str().unwrap();
        match mode {
            "missing" => {
                db.execute("DELETE FROM blobs WHERE id=?1", [root]).unwrap();
            }
            "tampered" => {
                db.execute("UPDATE blobs SET data=X'00' WHERE id=?1", [root])
                    .unwrap();
            }
            _ => {
                db.execute(
                    "UPDATE blobs SET data=zeroblob(?2) WHERE id=?1",
                    rusqlite::params![root, hide_session::SESSION_LINE_LIMIT_BYTES + 1],
                )
                .unwrap();
            }
        }
        drop(db);
        assert!(fixture.read(None).is_err(), "{mode}");
    }
}

#[test]
fn simulated_native_user_is_injected_and_not_a_human_turn() {
    let fixture = Fixture::new();
    fixture.root("second");
    let result = fixture.read(None).unwrap();
    assert_eq!(result.parsed.events.len(), 4);
    assert_eq!(result.parsed.events[2].kind, EventKind::Injected);
    assert!(result.parsed.events[2].is_provider_injected());
    assert!(result.parsed.turn_marks.is_empty());
}

#[test]
fn read_budget_is_a_refusal_when_native_headers_do_not_fit() {
    let fixture = Fixture::new();
    assert!(
        cursor::read(
            fixture.home.path(),
            &fixture.path,
            ID,
            fixture.cwd.to_str().unwrap(),
            None,
            1
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn linked_store_and_sidecar_are_not_reader_authority() {
    use std::os::unix::fs::symlink;
    for name in ["store.db", "meta.json"] {
        let fixture = Fixture::new();
        let path = fixture.path.with_file_name(name);
        let outside = fixture.home.path().join(format!("outside-{name}"));
        std::fs::rename(&path, &outside).unwrap();
        symlink(&outside, &path).unwrap();
        assert!(fixture.read(None).is_err());
    }
}

#[test]
fn a_live_wal_uses_its_committed_root_without_creating_files() {
    let fixture = Fixture::new();
    let writer = Connection::open(&fixture.path).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    let _: usize = writer
        .query_row("SELECT length(value) FROM meta WHERE key='0'", [], |row| {
            row.get(0)
        })
        .unwrap();
    fixture.root("append");
    let files = || {
        std::fs::read_dir(fixture.path.parent().unwrap())
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let before = files();
    // The native CLI is a different process. A writer and reader in one
    // process share SQLite's existing writable wal-index mapping, even if
    // the later reader requests readonly_shm. Keep the actual process boundary.
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "cursor::wal_reader_role", "--nocapture"])
        .env("CURSOR_WAL_READER_FIXTURE", fixture.home.path());
    let result = hide_platform::process::run_to_end(
        &mut command,
        std::time::Duration::from_secs(30),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(result.code, Some(0), "{} {}", result.stdout, result.stderr);
    assert!(result.stdout.contains("wal committed root read"));
    assert_eq!(files(), before);
    drop(writer);
}

#[test]
fn wal_reader_role() {
    let Some(root) = std::env::var_os("CURSOR_WAL_READER_FIXTURE") else {
        return;
    };
    let home = PathBuf::from(root);
    let cwd = home.join("project");
    let bucket = format!("{:x}", md5::compute(cwd.to_str().unwrap().as_bytes()));
    let path = home
        .join(cursor::SESSIONS)
        .join(bucket)
        .join(ID)
        .join("store.db");
    let result = cursor::read(
        &home,
        &path,
        ID,
        cwd.to_str().unwrap(),
        None,
        hide_session::SESSION_INCREMENT_READ_LIMIT_BYTES,
    )
    .unwrap();
    assert_eq!(result.parsed.events.len(), 3);
    assert_eq!(result.parsed.events[2].text, "Later answer");
    println!("wal committed root read");
}

#[test]
fn closed_checkpointed_wal_store_does_not_create_companions() {
    let fixture = Fixture::new();
    let writer = Connection::open(&fixture.path).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    let _: usize = writer
        .query_row("SELECT length(value) FROM meta WHERE key='0'", [], |row| {
            row.get(0)
        })
        .unwrap();
    drop(writer);
    assert!(!fixture.path.with_file_name("store.db-wal").exists());
    assert!(!fixture.path.with_file_name("store.db-shm").exists());
    let before = std::fs::read(&fixture.path).unwrap();
    assert_eq!(fixture.read(None).unwrap().parsed.events.len(), 2);
    assert_eq!(std::fs::read(&fixture.path).unwrap(), before);
    assert!(!fixture.path.with_file_name("store.db-wal").exists());
    assert!(!fixture.path.with_file_name("store.db-shm").exists());
}

#[test]
fn same_path_database_replacement_resets_native_checkpoint() {
    let fixture = Fixture::new();
    let first = fixture.read(None).unwrap();
    let replacement = fixture.path.with_extension("new");
    std::fs::copy(&fixture.path, &replacement).unwrap();
    std::fs::rename(replacement, &fixture.path).unwrap();
    let result = fixture.read(Some(first.checkpoint)).unwrap();
    assert_eq!(result.parsed.rescan_reason, Some(RescanReason::Replaced));
    assert_eq!(result.parsed.events.len(), 2);
}
