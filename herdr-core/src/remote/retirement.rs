//! B26 preflight before even the first helper staging file is written.
//! The authenticated SFTP channel reads only its account's ledgers and the
//! checkout paths already registered on that device. No helper version or
//! scripting interpreter is required.

use super::{EstablishError, FileAttributes, OpenFlags, RawSftpSession, SftpError, StatusCode};
use hide_kit::retirement_inspection::{
    self as predicates, MAX_ENTRIES, MAX_LEDGER_BYTES, MAX_STATE_BYTES,
};
use serde_json::Value;

pub(super) struct Locations {
    legacy_home: Option<String>,
    state_dir: Option<String>,
    xdg_state_home: Option<String>,
}

impl Locations {
    pub(super) fn from_environment(output: &str) -> Result<Self, EstablishError> {
        let values = output
            .strip_suffix('\n')
            .unwrap_or(output)
            .split('\n')
            .collect::<Vec<_>>();
        if values.len() != 3
            || values
                .iter()
                .any(|value| value.len() > 4096 || value.chars().any(char::is_control))
        {
            return Err(refused("the SSH environment paths are unreadable"));
        }
        let optional = |value: &str| (!value.is_empty()).then(|| value.to_owned());
        Ok(Self {
            legacy_home: optional(values[0]),
            state_dir: optional(values[1]),
            xdg_state_home: optional(values[2]),
        })
    }
}

fn refused(reason: impl std::fmt::Display) -> EstablishError {
    EstablishError::Install(format!(
        "Device retirement preflight refused: {reason}; inspect or finish the named run, request or watch and retry; no helper was uploaded"
    ))
}

fn absolute(path: &str, home: &str) -> Result<String, EstablishError> {
    let path = if let Some(relative) = path.strip_prefix("~/") {
        format!("{}/{relative}", home.trim_end_matches('/'))
    } else {
        path.to_owned()
    };
    if !path.starts_with('/')
        || path.len() > 4096
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(refused("an inspection path is not an absolute plain path"));
    }
    Ok(path.trim_end_matches('/').to_owned())
}

/// Checks every parent without following symlinks. Root-owned sticky system
/// ancestors are allowed, but no link can redirect a run or ledger read.
async fn directory(raw: &RawSftpSession, path: &str, owner: u32) -> Result<bool, EstablishError> {
    let mut current = String::new();
    for component in path.split('/').filter(|part| !part.is_empty()) {
        current.push('/');
        current.push_str(component);
        let attrs = match raw.lstat(&current).await {
            Ok(found) => found.attrs,
            Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
                return Ok(false);
            }
            Err(error) => return Err(refused(format!("{current} cannot be inspected: {error}"))),
        };
        let mode = attrs
            .permissions
            .ok_or_else(|| refused(format!("{current} has no reported permissions")))?;
        let shared = mode & 0o022 != 0;
        let safe_system = attrs.uid == Some(0) && (!shared || mode & 0o1000 != 0);
        if !attrs.is_dir() || (!safe_system && (attrs.uid != Some(owner) || shared)) {
            return Err(refused(format!(
                "{current} is not an owned directory safe to inspect"
            )));
        }
    }
    Ok(true)
}

async fn json(
    raw: &RawSftpSession,
    path: &str,
    owner: u32,
    limit: u64,
) -> Result<Option<Value>, EstablishError> {
    let (parent, _) = path
        .rsplit_once('/')
        .ok_or_else(|| refused("an inspection path has no parent"))?;
    if !directory(raw, parent, owner).await? {
        return Ok(None);
    }
    let attrs = match raw.lstat(path).await {
        Ok(found) => found.attrs,
        Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
            return Ok(None);
        }
        Err(error) => return Err(refused(format!("{path} cannot be inspected: {error}"))),
    };
    if !super::private_file(&attrs, owner) || attrs.size.is_none_or(|size| size > limit) {
        return Err(refused(format!(
            "{path} is not an owned regular file within the read bound"
        )));
    }
    let handle = raw
        .open(path, OpenFlags::READ, FileAttributes::empty())
        .await
        .map_err(|error| refused(format!("{path} cannot be opened for inspection: {error}")))?
        .handle;
    let result = async {
        let opened = raw.fstat(&handle).await.map_err(refused)?.attrs;
        if !super::private_file(&opened, owner) || opened.size != attrs.size {
            return Err(refused(format!("{path} changed while being opened")));
        }
        let mut bytes = Vec::new();
        loop {
            let length = (limit + 1 - bytes.len() as u64).min(32 * 1024) as u32;
            let chunk = match raw.read(&handle, bytes.len() as u64, length).await {
                Ok(data) => data.data,
                Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => break,
                Err(error) => return Err(refused(format!("{path} cannot be read: {error}"))),
            };
            if chunk.is_empty() {
                break;
            }
            bytes.extend(chunk);
            if bytes.len() as u64 > limit {
                return Err(refused(format!("{path} exceeds the read bound")));
            }
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| refused(format!("{path} is unreadable: {error}")))
    }
    .await;
    let _ = raw.close(handle).await;
    result
}

async fn entries(
    raw: &RawSftpSession,
    path: &str,
    owner: u32,
) -> Result<Vec<russh_sftp::protocol::File>, EstablishError> {
    if !directory(raw, path, owner).await? {
        return Ok(Vec::new());
    }
    let handle = raw.opendir(path).await.map_err(refused)?.handle;
    let result = async {
        let mut entries = Vec::new();
        loop {
            let batch = match raw.readdir(&handle).await {
                Ok(batch) => batch.files,
                Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => break,
                Err(error) => return Err(refused(error)),
            };
            if batch.is_empty() {
                break;
            }
            for entry in batch {
                if matches!(entry.filename.as_str(), "." | "..") {
                    continue;
                }
                if entry.filename.is_empty()
                    || entry.filename.len() > 4096
                    || entry.filename.contains('/')
                    || entry.filename.chars().any(char::is_control)
                {
                    return Err(refused("a directory entry has an unusable name"));
                }
                if entries.len() >= MAX_ENTRIES {
                    return Err(refused(format!("{path} exceeds the entry bound")));
                }
                entries.push(entry);
            }
        }
        Ok(entries)
    }
    .await;
    let _ = raw.close(handle).await;
    result
}

pub(super) async fn preflight(
    raw: &RawSftpSession,
    home: &str,
    owner: u32,
    projects: &[String],
    locations: &Locations,
) -> Result<(), EstablishError> {
    let mut homes = vec![format!("{home}/.hide/hcoord"), format!("{home}/.hcoord")];
    if let Some(relocated) = &locations.legacy_home {
        let relocated = absolute(relocated, home)?;
        if !homes.contains(&relocated) {
            homes.push(relocated);
        }
    }
    if let Some(progress) = json(
        raw,
        &format!("{home}/.hide/kit/coordination-retirement.json"),
        owner,
        64 * 1024,
    )
    .await?
        && predicates::completed(&progress, &homes).map_err(refused)?
    {
        return Ok(());
    }
    for legacy_home in &homes {
        directory(raw, legacy_home, owner).await?;
        if let Some(ledger) = json(
            raw,
            &format!("{legacy_home}/ledger.json"),
            owner,
            MAX_LEDGER_BYTES,
        )
        .await?
        {
            predicates::legacy_ledger(&ledger).map_err(refused)?;
        }
    }
    directory(raw, &format!("{home}/.hide/kit/hcoord"), owner).await?;
    let state = if let Some(state) = &locations.state_dir {
        absolute(state, home)?
    } else if let Some(xdg) = &locations.xdg_state_home {
        format!("{}/hide", absolute(xdg, home)?)
    } else {
        format!("{home}/.hide/state")
    };
    if let Some(ledger) = json(
        raw,
        &format!("{state}/delivery-ledger.json"),
        owner,
        MAX_LEDGER_BYTES,
    )
    .await?
    {
        predicates::delivery_ledger(&ledger).map_err(refused)?;
    }
    let registry = format!("{home}/.sasu/supervisor");
    let mut latest = "index.json".to_owned();
    let mut revision: Option<String> = None;
    for entry in entries(raw, &registry, owner).await? {
        if let Some(suffix) = entry.filename.strip_prefix("index.json.revision-")
            && suffix.len() == 12
            && suffix.bytes().all(|byte| byte.is_ascii_digit())
            && revision
                .as_ref()
                .is_none_or(|known| suffix > known.as_str())
        {
            revision = Some(suffix.to_owned());
            latest = entry.filename;
        }
    }
    if let Some(index) = json(raw, &format!("{registry}/{latest}"), owner, MAX_STATE_BYTES).await? {
        for path in predicates::supervisor_states(&index).map_err(refused)? {
            let path = absolute(path, home)?;
            let run = json(raw, &path, owner, MAX_STATE_BYTES)
                .await?
                .ok_or_else(|| refused("an indexed run state is missing"))?;
            predicates::indexed_run(&run).map_err(refused)?;
        }
    }
    if projects.len() > MAX_ENTRIES {
        return Err(refused("registered checkouts exceed the entry bound"));
    }
    for project in projects {
        let project = absolute(project, home)?;
        let runs = format!("{project}/agents/runs");
        for entry in entries(raw, &runs, owner).await? {
            if entry.attrs.is_symlink() {
                return Err(refused("a registered run folder is a symlink"));
            }
            if !entry.attrs.is_dir() {
                continue;
            }
            if let Some(run) = json(
                raw,
                &format!("{runs}/{}/state.json", entry.filename),
                owner,
                MAX_STATE_BYTES,
            )
            .await?
            {
                predicates::checkout_run(&run).map_err(refused)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh_sftp::protocol::{Attrs, Data, File, Handle, Name, Status};
    use std::collections::{BTreeMap, HashSet};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    const HOME: &str = "/fixture-home";
    const OWNER: u32 = 42;

    /// A private in-memory SFTP account. Every attempted write is counted;
    /// there is no SSH connection, OS process or operator path in this test.
    struct Account {
        files: BTreeMap<String, (FileAttributes, Vec<u8>)>,
        listed: HashSet<String>,
        mutations: Arc<AtomicUsize>,
    }

    impl Account {
        fn new() -> Self {
            let mut account = Self {
                files: BTreeMap::new(),
                listed: HashSet::new(),
                mutations: Arc::new(AtomicUsize::new(0)),
            };
            account
                .files
                .insert(HOME.into(), (attributes(true, 0), Vec::new()));
            account
        }

        fn add(&mut self, path: &str, value: Value) {
            let bytes = serde_json::to_vec(&value).unwrap();
            let mut parent = path.rsplit_once('/').unwrap().0;
            while !parent.is_empty() {
                self.files
                    .entry(parent.into())
                    .or_insert((attributes(true, 0), Vec::new()));
                parent = parent.rsplit_once('/').unwrap().0;
            }
            self.files
                .insert(path.into(), (attributes(false, bytes.len() as u64), bytes));
        }
    }

    fn attributes(directory: bool, size: u64) -> FileAttributes {
        FileAttributes {
            uid: Some(OWNER),
            size: Some(size),
            permissions: Some(if directory { 0o040700 } else { 0o100600 }),
            ..FileAttributes::empty()
        }
    }

    impl russh_sftp::server::Handler for Account {
        type Error = StatusCode;
        fn unimplemented(&self) -> Self::Error {
            self.mutations.fetch_add(1, Ordering::SeqCst);
            StatusCode::OpUnsupported
        }
        async fn realpath(&mut self, id: u32, _path: String) -> Result<Name, Self::Error> {
            Ok(Name {
                id,
                files: vec![File::dummy(HOME)],
            })
        }
        async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
            let attrs = self
                .files
                .get(&path)
                .ok_or(StatusCode::NoSuchFile)?
                .0
                .clone();
            Ok(Attrs { id, attrs })
        }
        async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
            self.lstat(id, handle).await
        }
        async fn open(
            &mut self,
            id: u32,
            filename: String,
            flags: OpenFlags,
            _attrs: FileAttributes,
        ) -> Result<Handle, Self::Error> {
            if flags.bits() != OpenFlags::READ.bits() {
                return Err(self.unimplemented());
            }
            self.files.get(&filename).ok_or(StatusCode::NoSuchFile)?;
            Ok(Handle {
                id,
                handle: filename,
            })
        }
        async fn read(
            &mut self,
            id: u32,
            handle: String,
            offset: u64,
            len: u32,
        ) -> Result<Data, Self::Error> {
            let bytes = &self.files.get(&handle).ok_or(StatusCode::NoSuchFile)?.1;
            let offset = offset as usize;
            if offset >= bytes.len() {
                return Err(StatusCode::Eof);
            }
            Ok(Data {
                id,
                data: bytes[offset..bytes.len().min(offset + len as usize)].to_vec(),
            })
        }
        async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
            Ok(Status {
                id,
                status_code: StatusCode::Ok,
                error_message: String::new(),
                language_tag: String::new(),
            })
        }
        async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
            if !self
                .files
                .get(&path)
                .ok_or(StatusCode::NoSuchFile)?
                .0
                .is_dir()
            {
                return Err(StatusCode::Failure);
            }
            Ok(Handle { id, handle: path })
        }
        async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
            if !self.listed.insert(handle.clone()) {
                return Err(StatusCode::Eof);
            }
            let files = self
                .files
                .iter()
                .filter_map(|(path, (attrs, _))| {
                    let (parent, name) = path.rsplit_once('/')?;
                    (parent == handle).then(|| File::new(name, attrs.clone()))
                })
                .collect();
            Ok(Name { id, files })
        }
    }

    fn inspect(
        account: Account,
        projects: &[String],
        installation: bool,
    ) -> (Result<(), EstablishError>, usize) {
        let mutations = Arc::clone(&account.mutations);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async {
            let (client, server) = tokio::io::duplex(256 * 1024);
            let task = tokio::spawn(russh_sftp::server::run(server, account));
            let raw = RawSftpSession::new(client);
            let locations = Locations::from_environment("\n\n\n").unwrap();
            let result = if installation {
                // Exercise the real install ordering. A blocker must return
                // before this deliberately empty payload can create a folder.
                let payload = super::super::Payload {
                    files: Vec::new(),
                    missing: Vec::new(),
                };
                super::super::install(&raw, "~/.hide/host-helper", &payload, projects, &locations)
                    .await
                    .map(|_| ())
            } else {
                raw.init().await.unwrap();
                preflight(&raw, HOME, OWNER, projects, &locations).await
            };
            let _ = raw.close_session();
            drop(raw);
            task.await.unwrap();
            result
        });
        (result, mutations.load(Ordering::SeqCst))
    }

    #[test]
    fn active_registered_run_refuses_before_any_remote_install_mutation() {
        let mut account = Account::new();
        account.add("/checkout/agents/runs/task/state.json", serde_json::json!({"schema":"sasu.implement.state.v11.stateless-verification", "status":"active"}));
        let (result, mutations) = inspect(account, &["/checkout".into()], true);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("a sasu run is active")
        );
        assert_eq!(mutations, 0, "the blocker must precede staging and mkdir");
    }

    #[test]
    fn newest_supervisor_revision_is_read_before_remote_staging() {
        let mut account = Account::new();
        account.add("/fixture-home/.sasu/supervisor/index.json", serde_json::json!({"schema":"sasu.supervisor.index.v1", "entries":[], "coordinated":[]}));
        account.add("/fixture-home/.sasu/supervisor/index.json.revision-000000000002", serde_json::json!({"schema":"sasu.supervisor.index.v1", "entries":[], "coordinated":[{"statePath":"/run/state.json", "runInstanceId":"run"}]}));
        account.add("/run/state.json", serde_json::json!({"status":"active"}));
        let (result, mutations) = inspect(account, &[], true);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("a sasu run is active")
        );
        assert_eq!(mutations, 0);
    }

    #[test]
    fn open_request_and_unknown_run_status_refuse_without_remote_writes() {
        for (path, state) in [
            (
                "/fixture-home/.hcoord/ledger.json",
                serde_json::json!({"schema":"hcoord.ledger.v1", "requests":{"request":{"status":"open"}}, "watches":{}}),
            ),
            (
                "/checkout/agents/runs/task/state.json",
                serde_json::json!({"schema":"sasu.implement.state.v10", "status":"unknown"}),
            ),
        ] {
            let mut account = Account::new();
            account.add(path, state);
            let (result, mutations) = inspect(account, &["/checkout".into()], true);
            assert!(result.is_err());
            assert_eq!(mutations, 0);
        }
    }

    #[test]
    fn private_clean_account_and_unrelated_artifacts_pass_read_only() {
        let mut account = Account::new();
        account.add(
            "/checkout/agents/runs/task/state.json",
            serde_json::json!({"schema":"another-tool.v1"}),
        );
        let (result, mutations) = inspect(account, &["/checkout".into()], false);
        result.unwrap();
        assert_eq!(mutations, 0);
    }

    #[test]
    fn symlink_and_oversized_run_files_fail_closed() {
        for permissions in [0o120600, 0o100600] {
            let mut account = Account::new();
            let path = "/checkout/agents/runs/task/state.json";
            account.add(path, serde_json::json!({"status":"retired"}));
            let (attrs, _) = account.files.get_mut(path).unwrap();
            attrs.permissions = Some(permissions);
            attrs.size = Some(MAX_STATE_BYTES + 1);
            let (result, mutations) = inspect(account, &["/checkout".into()], true);
            assert!(result.is_err());
            assert_eq!(mutations, 0);
        }
    }

    #[test]
    fn completed_receipt_preserves_idempotency_and_bad_receipts_refuse() {
        for (progress, allowed) in [
            (
                serde_json::json!({"homes":[], "step":"rename ledger", "failure":null, "complete":true}),
                true,
            ),
            (
                serde_json::json!({"homes":[["/unowned", "/unowned.retired-2026-01-01"]], "step":"rename ledger", "failure":null, "complete":true}),
                false,
            ),
        ] {
            let mut account = Account::new();
            account.add(
                "/fixture-home/.hide/kit/coordination-retirement.json",
                progress,
            );
            account.add(
                "/checkout/agents/runs/task/state.json",
                serde_json::json!({"status":"active"}),
            );
            let (result, mutations) = inspect(account, &["/checkout".into()], false);
            assert_eq!(result.is_ok(), allowed);
            assert_eq!(mutations, 0);
        }
    }

    #[test]
    fn inspection_paths_are_plain_and_environment_is_exact() {
        assert!(Locations::from_environment("\n\n\n").is_ok());
        assert!(Locations::from_environment("\n\n").is_err());
        assert!(absolute("~/../other", HOME).is_err());
        assert!(absolute("relative", HOME).is_err());
        assert_eq!(
            absolute("~/checkout", HOME).unwrap(),
            "/fixture-home/checkout"
        );
    }
}
