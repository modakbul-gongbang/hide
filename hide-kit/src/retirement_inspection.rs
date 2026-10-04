//! Read-only predicates shared by local kit and the pre-upload SFTP inspection.
//! They parse occupancy only; no legacy contents are imported or rewritten.

use serde_json::Value;

pub const MAX_LEDGER_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_STATE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 1024;

pub fn legacy_ledger(ledger: &Value) -> Result<(), String> {
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
            let status = value["status"]
                .as_str()
                .ok_or_else(|| format!("legacy {table} has no status; retirement has not begun"))?;
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
    Ok(())
}

pub fn delivery_ledger(ledger: &Value) -> Result<(), String> {
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
                "pending" | "delivered" | "acknowledged" | "cancelled" | "expired" | "undelivered"
            )
        ) || !letter["waiting_answer"].is_boolean()
        {
            return Err("Hide request status cannot be inspected; retirement has not begun".into());
        }
    }
    if letters
        .iter()
        .any(|value| value["state"] == "pending" || value["waiting_answer"] == true)
    {
        return Err("Hide has open requests; finish or close them, then retry retirement".into());
    }
    if !watches.is_empty() {
        return Err("Hide has active watches; stop them, then retry retirement".into());
    }
    Ok(())
}

/// State paths from the authoritative latest supervisor revision.
pub fn supervisor_states(index: &Value) -> Result<Vec<&str>, String> {
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
    coordinated
        .iter()
        .map(|entry| {
            entry["runInstanceId"]
                .as_str()
                .ok_or("sasu run has no identity; inspect its registry before retirement")?;
            entry["statePath"].as_str().ok_or_else(|| {
                "sasu run has no state path; inspect its registry before retirement".into()
            })
        })
        .collect()
}

pub fn indexed_run(state: &Value) -> Result<(), String> {
    match state["status"].as_str() {
        Some("retired") => Ok(()),
        Some("active") => Err("a sasu run is active; retire it and retry retirement".into()),
        _ => Err("sasu run status cannot be inspected; retirement has not begun".into()),
    }
}

/// Other tools keep artifacts under agents/runs too. Only Sasu's source marker
/// makes an unknown status its unreadable run; an active status always blocks.
pub fn checkout_run(state: &Value) -> Result<(), String> {
    if state["status"] == "active"
        || state["schema"]
            .as_str()
            .is_some_and(|schema| schema.starts_with("sasu.implement.state.v"))
    {
        indexed_run(state)?;
    }
    Ok(())
}

/// A bounded receipt can bypass occupancy only after retirement completed.
pub fn completed(progress: &Value, homes: &[String]) -> Result<bool, String> {
    let complete = progress["complete"]
        .as_bool()
        .ok_or("retirement progress is unreadable; inspect it before retrying")?;
    progress["step"]
        .as_str()
        .ok_or("retirement progress is unreadable; inspect it before retrying")?;
    if !progress["failure"].is_null() && !progress["failure"].is_string() {
        return Err("retirement progress is unreadable; inspect it before retrying".into());
    }
    let pairs = progress["homes"]
        .as_array()
        .ok_or("retirement progress is unreadable; inspect it before retrying")?;
    for pair in pairs {
        let pair = pair
            .as_array()
            .filter(|pair| pair.len() == 2)
            .ok_or("retirement progress names an unexpected folder; inspect it before retrying")?;
        let source = pair[0]
            .as_str()
            .ok_or("retirement progress is unreadable")?;
        let destination = pair[1]
            .as_str()
            .ok_or("retirement progress is unreadable")?;
        let Some((parent, name)) = source.rsplit_once('/') else {
            return Err(
                "retirement progress names an unexpected folder; inspect it before retrying".into(),
            );
        };
        if !homes.iter().any(|home| home == source)
            || !destination.starts_with(&format!("{parent}/{name}.retired-"))
            || destination[parent.len() + 1..].contains('/')
        {
            return Err(
                "retirement progress names an unexpected folder; inspect it before retrying".into(),
            );
        }
    }
    Ok(complete)
}
