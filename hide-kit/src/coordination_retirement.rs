//! One-release retirement, never a ledger migration or a compatibility command.
//! Reads of the old ledger are bounded preflight predicates only.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{KitTarget, Observed};

const MAX_LEDGER_BYTES: u64 = 64 * 1024 * 1024;
const MAX_STATE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 1024;

#[derive(Default, Serialize, Deserialize)]
struct Progress {
    homes: Vec<(PathBuf, PathBuf)>,
    step: String,
    failure: Option<String>,
    complete: bool,
}

fn path(target: &KitTarget) -> PathBuf {
    crate::kit_state_dir(&target.home).join("coordination-retirement.json")
}

fn homes(target: &KitTarget) -> Vec<PathBuf> {
    let mut paths = vec![
        target.home.join(".hide/hcoord"),
        target.home.join(".hcoord"),
    ];
    if let Some(home) = &target.legacy_coordination_home
        && !paths.contains(home)
    {
        paths.push(home.clone());
    }
    paths
}

fn read_json(path: &Path, limit: u64) -> Result<Option<Value>, String> {
    let file = match hide_platform::fs::private::open_own_file(path, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "{} cannot be inspected: {error}; resolve it before retrying retirement",
                path.display()
            ));
        }
    };
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "{} exceeds the preflight size bound; inspect it before retrying retirement",
            path.display()
        ));
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        format!(
            "{} cannot be inspected: {error}; no retirement can begin",
            path.display()
        )
    })
}

fn load(target: &KitTarget) -> Result<Option<Progress>, String> {
    crate::record::private_state_dir(&target.home, false)?;
    let progress: Option<Progress> = read_json(&path(target), 64 * 1024)?
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| format!("retirement progress is unreadable: {error}"))?;
    if let Some(progress) = &progress {
        for (source, destination) in &progress.homes {
            let prefix = format!(
                "{}.retired-",
                source.file_name().unwrap_or_default().to_string_lossy()
            );
            if !homes(target).contains(source)
                || source.parent() != destination.parent()
                || !destination
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
            {
                return Err(
                    "retirement progress names an unexpected folder; inspect it before retrying"
                        .into(),
                );
            }
        }
    }
    Ok(progress)
}

fn save(target: &KitTarget, progress: &Progress) -> Result<(), String> {
    let bytes = serde_json::to_vec(progress).map_err(|error| error.to_string())?;
    crate::record::private_state_dir(&target.home, true)?;
    crate::write_atomically(&path(target), &bytes, hide_platform::fs::Access::Private)
}

/// No file creation, permissions change, launchctl, daemon RPC or install work.
pub fn preflight(target: &KitTarget) -> Result<(), String> {
    if load(target)?.is_some_and(|progress| progress.complete) {
        return Ok(());
    }
    for home in homes(target) {
        inspect_directory(&home)?;
        let Some(ledger) = read_json(&home.join("ledger.json"), MAX_LEDGER_BYTES)? else {
            continue;
        };
        if ledger["schema"] != "hcoord.ledger.v1" {
            return Err("legacy coordination ledger has an unknown schema; inspect it before retrying retirement".into());
        }
        for (table, active, closed) in [
            ("requests", "open", &["answered", "canceled"][..]),
            ("watches", "active", &["stopped"][..]),
        ] {
            let entries = ledger[table].as_object().ok_or_else(|| {
                format!("legacy {table} cannot be inspected; retirement has not begun")
            })?;
            for value in entries.values() {
                let status = value["status"].as_str().ok_or_else(|| {
                    format!("legacy {table} has no status; retirement has not begun")
                })?;
                if status == active {
                    return Err(format!(
                        "legacy coordination has {active} {table}; finish or close them, then retry retirement"
                    ));
                }
                if !closed.contains(&status) {
                    return Err(format!(
                        "legacy {table} has an unknown status; inspect it before retrying retirement"
                    ));
                }
            }
        }
    }
    inspect_directory(&crate::kit_state_dir(&target.home).join("hcoord"))?;
    if let Some(ledger) = read_json(
        &crate::layout::delivery_ledger(&crate::layout::state_dir_from_process(&target.home)),
        MAX_LEDGER_BYTES,
    )? {
        if ledger["version"] != 1 {
            return Err(
                "Hide delivery ledger has an unknown version; inspect it before retirement".into(),
            );
        }
        let letters = ledger["letters"]
            .as_array()
            .ok_or("Hide requests cannot be inspected; retirement has not begun")?;
        let watches = ledger["watches"]
            .as_array()
            .ok_or("Hide watches cannot be inspected; retirement has not begun")?;
        for letter in letters {
            if !matches!(
                letter["state"].as_str(),
                Some(
                    "pending"
                        | "delivered"
                        | "acknowledged"
                        | "cancelled"
                        | "expired"
                        | "undelivered"
                )
            ) || !letter["waiting_answer"].is_boolean()
            {
                return Err(
                    "Hide request status cannot be inspected; retirement has not begun".into(),
                );
            }
        }
        if letters
            .iter()
            .any(|value| value["state"] == "pending" || value["waiting_answer"] == true)
        {
            return Err(
                "Hide has open requests; finish or close them, then retry retirement".into(),
            );
        }
        if !watches.is_empty() {
            return Err("Hide has active watches; stop them, then retry retirement".into());
        }
    }
    sasu_preflight(&target.home)?;
    for project in &target.retirement_projects {
        project_preflight(project)?;
    }
    Ok(())
}

fn inspect_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("a legacy coordination folder is not an owned directory; inspect it before retirement".into());
            }
            let owned = hide_platform::fs::private::owned_by_current_user(path)
                .map_err(|error| error.to_string())?;
            let writable = hide_platform::fs::private::others_can_modify(path)
                .map_err(|error| error.to_string())?;
            if !owned || writable {
                return Err("a legacy coordination folder can be changed by another account; inspect it before retirement".into());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "legacy coordination folder cannot be inspected: {error}"
        )),
    }
}

fn sasu_preflight(home: &Path) -> Result<(), String> {
    let directory = home.join(".sasu/supervisor");
    let mut latest = directory.join("index.json");
    let mut revision = None;
    match fs::read_dir(&directory) {
        Ok(entries) => {
            for (count, entry) in entries.enumerate() {
                if count >= MAX_ENTRIES {
                    return Err("sasu supervisor folder exceeds the preflight entry bound; inspect it before retirement".into());
                }
                let entry = entry.map_err(|error| error.to_string())?;
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                let Some(suffix) = name.strip_prefix("index.json.revision-") else {
                    continue;
                };
                if suffix.len() == 12
                    && suffix.bytes().all(|byte| byte.is_ascii_digit())
                    && revision
                        .as_ref()
                        .is_none_or(|known: &String| suffix > known.as_str())
                {
                    revision = Some(suffix.to_owned());
                    latest = entry.path();
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "sasu registry cannot be inspected: {error}; retirement has not begun"
            ));
        }
    }
    let Some(index) = read_json(&latest, MAX_STATE_BYTES)? else {
        return Ok(());
    };
    if index["schema"] != "sasu.supervisor.index.v1" {
        return Err("sasu registry has an unknown schema; inspect it before retirement".into());
    }
    if !index["tickExecutor"].is_null() {
        return Err(
            "sasu supervisor still has an active tick; stop it and retry retirement".into(),
        );
    }
    let entries = index["entries"]
        .as_array()
        .ok_or("sasu indexed runs cannot be inspected")?;
    if !entries.is_empty() {
        return Err(
            "sasu supervisor still owns indexed runs; retire them and retry retirement".into(),
        );
    }
    let coordinated = if index.get("coordinated").is_none() {
        &[][..]
    } else {
        index["coordinated"]
            .as_array()
            .ok_or("sasu coordinated runs cannot be inspected")?
            .as_slice()
    };
    if coordinated.len() > MAX_ENTRIES {
        return Err("sasu runs exceed the preflight entry bound".into());
    }
    for entry in coordinated {
        let state = entry["statePath"]
            .as_str()
            .ok_or("sasu run has no state path; inspect its registry before retirement")?;
        entry["runInstanceId"]
            .as_str()
            .ok_or("sasu run has no identity; inspect its registry before retirement")?;
        let state = read_json(Path::new(state), MAX_STATE_BYTES)?
            .ok_or("a sasu run state is missing; inspect its registry before retirement")?;
        if !matches!(state["status"].as_str(), Some("active" | "retired")) {
            return Err("sasu run status cannot be inspected; retirement has not begun".into());
        }
        if state["status"] == "active" {
            return Err("a sasu run is active; retire it and retry retirement".into());
        }
    }
    Ok(())
}

fn project_preflight(project: &Path) -> Result<(), String> {
    let directory = project.join("agents/runs");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "registered checkout runs cannot be inspected: {error}"
            ));
        }
    };
    for (count, entry) in entries.enumerate() {
        if count >= MAX_ENTRIES {
            return Err("registered checkout runs exceed the preflight entry bound".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        if let Some(state) = read_json(&entry.path().join("state.json"), MAX_STATE_BYTES)?
            && state["status"] == "active"
        {
            return Err(
                "a registered checkout has an active sasu run; retire it and retry retirement"
                    .into(),
            );
        }
    }
    Ok(())
}

pub(crate) fn observe(target: &KitTarget) -> Observed {
    match load(target) {
        Err(reason) => Observed::Blocked(reason),
        Ok(Some(progress)) if progress.complete => Observed::Current,
        Ok(Some(progress)) if progress.failure.is_some() => {
            Observed::Blocked(progress.failure.unwrap())
        }
        Ok(Some(progress)) => Observed::Stale(format!("retirement resumes at {}", progress.step)),
        Ok(None) => match preflight(target) {
            Err(reason) => Observed::Blocked(reason),
            Ok(()) => {
                Observed::Stale("legacy coordination retirement has not run on this machine".into())
            }
        },
    }
}

pub(crate) fn location(target: &KitTarget) -> PathBuf {
    path(target)
}

pub(crate) fn install(target: &KitTarget) -> Result<crate::Retirement, String> {
    preflight(target)?;
    let mut progress = load(target)?.unwrap_or_else(|| Progress {
        homes: homes(target)
            .into_iter()
            .filter(|home| home.exists())
            .map(|home| {
                let name = format!(
                    "{}.retired-{}",
                    home.file_name().unwrap_or_default().to_string_lossy(),
                    date()
                );
                let destination = home.with_file_name(name);
                (home, destination)
            })
            .collect(),
        ..Progress::default()
    });
    if progress.complete {
        return Ok(crate::Retirement::default());
    }
    let mut retirement = crate::Retirement::default();
    for step in [
        "stop daemon",
        "remove login agent",
        "remove owned links",
        "remove kit copy",
        "preserve ledger",
    ] {
        if target.stop.load(Ordering::Relaxed) {
            return Err("Hide is quitting; retirement resumes on the next pass".into());
        }
        progress.step = step.into();
        progress.failure = None;
        save(target, &progress)?;
        let outcome = match step {
            "stop daemon" => homes(target).iter().try_for_each(|home| stop_daemon(home)),
            "remove login agent" => homes(target)
                .iter()
                .try_for_each(|home| remove_agent(target, home)),
            "remove owned links" => remove_links(target).map(|report| {
                retirement = report;
            }),
            "remove kit copy" => remove_copy(target),
            _ => preserve(&progress),
        };
        if let Err(reason) = outcome {
            progress.failure = Some(format!(
                "{} failed: {reason}; resolve it and retry retirement",
                progress.step
            ));
            save(target, &progress)?;
            return Err(progress.failure.unwrap());
        }
    }
    progress.complete = true;
    progress.step = "complete".into();
    save(target, &progress).map(|()| retirement)
}

fn stop_daemon(home: &Path) -> Result<(), String> {
    let path = home.join("api.sock");
    let mut socket = match hide_platform::ipc::LocalStream::connect(&path) {
        Ok(socket) => socket,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(());
        }
        Err(error) => return Err(format!("daemon socket cannot be reached: {error}")),
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    socket
        .write_all(b"{\"version\":1,\"operation\":\"daemon.stop\",\"args\":{}}\n")
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err("daemon stop response deadline reached".into());
        }
        socket
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        let count = socket.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("daemon closed before acknowledging stop".into());
        }
        response.extend_from_slice(&chunk[..count]);
        if response.len() > 64 * 1024 {
            return Err("daemon stop response exceeded the size bound".into());
        }
        if let Some(newline) = response.iter().position(|byte| *byte == b'\n') {
            response.truncate(newline);
            break;
        }
    }
    let value: Value = serde_json::from_slice(&response).map_err(|error| error.to_string())?;
    if value["ok"] != true || value["value"]["stopped"] != true {
        return Err("daemon refused the stop request".into());
    }
    let end = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match hide_platform::ipc::LocalStream::connect(&path) {
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(format!("daemon exit cannot be confirmed: {error}")),
            Ok(_) if std::time::Instant::now() >= end => {
                return Err("daemon still answers after stop; retirement did not proceed".into());
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn label(home: &Path) -> Result<String, String> {
    let account = hide_platform::user_agents::account_home()
        .map_err(|error| format!("account home cannot be resolved: {error}"))?;
    if home == account.join(".hide/hcoord") {
        return Ok("com.hcoord.daemon".into());
    }
    let digest = Sha256::digest(home.to_string_lossy().as_bytes());
    Ok(format!(
        "com.hcoord.daemon.{}",
        &format!("{digest:x}")[..12]
    ))
}

fn remove_agent(target: &KitTarget, home: &Path) -> Result<(), String> {
    let label = label(home)?;
    target
        .user_agents
        .unload(&label, &target.home, &target.stop)
        .map_err(|error| error.to_string())?;
    remove_file(&hide_platform::user_agents::UserAgents::plist(
        &target.home,
        &label,
    ))
}

fn remove_links(target: &KitTarget) -> Result<crate::Retirement, String> {
    let mut folders = vec![target.cli_dir.clone(), target.home.join(".local/bin")];
    if let Some(path) = std::env::var_os("PATH") {
        folders.extend(std::env::split_paths(&path));
    }
    folders.sort();
    folders.dedup();
    for folder in folders {
        let link = folder.join("hcoord");
        let Ok(destination) = fs::read_link(&link) else {
            continue;
        };
        let destination = if destination.is_absolute() {
            destination
        } else {
            folder.join(destination)
        };
        if homes(target)
            .iter()
            .any(|home| destination == home.join("bin/hcoord"))
        {
            hide_platform::fs::link::remove_link(&link)
                .map_err(|error| format!("owned command link could not be removed: {error}"))?;
        }
    }
    let plugin = crate::labels::retire_hcoord_plugin(target);
    if !plugin.failures.is_empty() {
        return Err(plugin.failures.join("; "));
    }
    Ok(plugin)
}

fn remove_copy(target: &KitTarget) -> Result<(), String> {
    let path = crate::kit_state_dir(&target.home).join("hcoord");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("the kit copy is a link; inspect its ownership before retrying".into())
        }
        Ok(_) => {
            fs::remove_dir_all(path).map_err(|error| format!("kit copy cannot be removed: {error}"))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn preserve(progress: &Progress) -> Result<(), String> {
    for (home, destination) in &progress.homes {
        if home.exists() {
            if destination.exists() {
                return Err("the preserved ledger name already exists; keep both folders and resolve the collision before retrying".into());
            }
            fs::rename(home, destination)
                .map_err(|error| format!("ledger folder cannot be renamed: {error}"))?;
        } else if !destination.exists() {
            return Err("both the old ledger folder and its preserved destination are missing; inspect before retrying".into());
        }
    }
    Ok(())
}

fn remove_file(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn date() -> String {
    // Gregorian civil date from UTC days. No shell command or runtime dependency.
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86400;
    let z = days as i64 + 719468;
    let era = z / 146097;
    let day = z - era * 146097;
    let year_of_era = (day - day / 1460 + day / 36524 - day / 146096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_of_year = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_of_year + 2) / 5 + 1;
    let month = month_of_year + if month_of_year < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
