use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode, Output};

use herdr_core::fixture::{
    FIXTURE_PREFIX, FixtureManifest, FixtureWorkspaceRecord, ScalePlan, plan, plan_scale,
    validate_manifest,
};
use serde_json::Value;

const REMOTE_HERDR: &str = "~/.local/bin/herdr";

fn main() -> ExitCode {
    match execute(std::env::args().skip(1).collect()) {
        Ok(value) => match serde_json::to_string_pretty(&value) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(_) => {
                eprintln!("fixture result could not be encoded");
                ExitCode::from(1)
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn execute(args: Vec<String>) -> Result<Value, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(usage());
    };
    match command {
        "plan" => {
            let name = args.get(1).ok_or_else(usage)?;
            serde_json::to_value(plan(name)?)
                .map_err(|_| "fixture plan could not be encoded".to_owned())
        }
        "plan-scale" => {
            let request = parse_scale_request(&args[1..])?;
            serde_json::to_value(request).map_err(|_| "scale plan could not be encoded".to_owned())
        }
        "create" => {
            let request = parse_scale_request(&args[1..])?;
            let manifest_path = argument_value("--manifest", &args[1..])
                .ok_or_else(|| "create requires --manifest <path>".to_owned())?;
            let manifest = create(&request)?;
            save_manifest(Path::new(&manifest_path), &manifest)?;
            serde_json::to_value(manifest)
                .map_err(|_| "fixture manifest could not be encoded".to_owned())
        }
        "status" => {
            let manifest = load_manifest_argument(&args[1..])?;
            status(&manifest)
        }
        "cleanup" => {
            let manifest = load_manifest_argument(&args[1..])?;
            cleanup(&manifest)
        }
        name if name.starts_with(FIXTURE_PREFIX) => serde_json::to_value(plan(name)?)
            .map_err(|_| "fixture plan could not be encoded".to_owned()),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage: herdr-ide-fixture plan herdr-ide-verify-<name> | plan-scale/create herdr-ide-verify-<name> --target local|mini --cwd /tmp/herdr-ide-verify-... --workspaces N --panes N [--manifest path] | status/cleanup --manifest path".to_owned()
}

fn parse_scale_request(args: &[String]) -> Result<ScalePlan, String> {
    let name = args.first().ok_or_else(usage)?;
    let target =
        argument_value("--target", args).ok_or_else(|| "--target is required".to_owned())?;
    let cwd = argument_value("--cwd", args).ok_or_else(|| "--cwd is required".to_owned())?;
    if !cwd.starts_with(&format!("/tmp/{FIXTURE_PREFIX}")) {
        return Err(format!("fixture cwd must begin with /tmp/{FIXTURE_PREFIX}"));
    }
    let workspace_count = parse_count("--workspaces", args)?;
    let pane_count = parse_count("--panes", args)?;
    plan_scale(name, &target, &cwd, workspace_count, pane_count)
}

fn parse_count(flag: &str, args: &[String]) -> Result<usize, String> {
    argument_value(flag, args)
        .ok_or_else(|| format!("{flag} is required"))?
        .parse()
        .map_err(|_| format!("{flag} must be a positive integer"))
}

fn argument_value(flag: &str, args: &[String]) -> Option<String> {
    args.iter()
        .position(|argument| argument == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn create(plan: &ScalePlan) -> Result<FixtureManifest, String> {
    prepare_directory(&plan.target, &plan.cwd)?;
    let mut live = workspace_list(&plan.target)?;
    let mut records = Vec::with_capacity(plan.workspace_names.len());
    let extra_panes = plan.requested_pane_count - plan.requested_workspace_count;

    for (index, workspace_name) in plan.workspace_names.iter().enumerate() {
        let workspace_id = match find_workspace_id(&live, workspace_name) {
            Some(id) => id,
            None => {
                let output = run_herdr(
                    &plan.target,
                    &[
                        "workspace",
                        "create",
                        "--cwd",
                        &plan.cwd,
                        "--label",
                        workspace_name,
                        "--no-focus",
                    ],
                )?;
                find_string(&output, "workspace_id").ok_or_else(|| {
                    format!("workspace create did not return an id for {workspace_name}")
                })?
            }
        };
        live = workspace_list(&plan.target)?;
        let verified_label = find_workspace_label(&live, &workspace_id)
            .ok_or_else(|| format!("created workspace {workspace_id} was not observable"))?;
        if verified_label != *workspace_name || !verified_label.starts_with(FIXTURE_PREFIX) {
            return Err(format!("workspace ownership mismatch for {workspace_id}"));
        }

        let desired_panes = if index == 0 { 1 + extra_panes } else { 1 };
        let mut live_pane_ids = pane_ids(&plan.target, &workspace_id)?;
        while live_pane_ids.len() < desired_panes {
            let anchor = live_pane_ids
                .first()
                .cloned()
                .ok_or_else(|| format!("workspace {workspace_id} has no anchor pane"))?;
            run_herdr(
                &plan.target,
                &[
                    "pane",
                    "split",
                    "--pane",
                    &anchor,
                    "--direction",
                    "right",
                    "--cwd",
                    &plan.cwd,
                    "--no-focus",
                ],
            )?;
            live_pane_ids = pane_ids(&plan.target, &workspace_id)?;
        }
        if live_pane_ids.len() != desired_panes {
            return Err(format!(
                "workspace {workspace_id} has {} panes; expected {desired_panes}",
                live_pane_ids.len()
            ));
        }
        for pane_id in &live_pane_ids {
            run_herdr_command(
                &plan.target,
                &["pane", "run", pane_id, "/usr/bin/tail", "-f", "/dev/null"],
            )?;
        }
        records.push(FixtureWorkspaceRecord {
            workspace_name: workspace_name.clone(),
            workspace_id,
            pane_ids: live_pane_ids,
        });
    }

    let manifest = FixtureManifest {
        schema_version: 1,
        ownership_prefix: FIXTURE_PREFIX.to_owned(),
        target: plan.target.clone(),
        cwd: plan.cwd.clone(),
        workspaces: records,
    };
    validate_manifest(&manifest)?;
    let total_panes: usize = manifest
        .workspaces
        .iter()
        .map(|workspace| workspace.pane_ids.len())
        .sum();
    if manifest.workspaces.len() != plan.requested_workspace_count
        || total_panes != plan.requested_pane_count
    {
        return Err("live fixture totals did not converge to the requested scale".to_owned());
    }
    Ok(manifest)
}

fn status(manifest: &FixtureManifest) -> Result<Value, String> {
    validate_manifest(manifest)?;
    let live = workspace_list(&manifest.target)?;
    let mut verified = Vec::with_capacity(manifest.workspaces.len());
    for workspace in &manifest.workspaces {
        let label = find_workspace_label(&live, &workspace.workspace_id)
            .ok_or_else(|| format!("fixture workspace {} is absent", workspace.workspace_id))?;
        if label != workspace.workspace_name || !label.starts_with(FIXTURE_PREFIX) {
            return Err(format!(
                "fixture ownership changed for {}",
                workspace.workspace_id
            ));
        }
        let panes = pane_ids(&manifest.target, &workspace.workspace_id)?;
        verified.push(serde_json::json!({
            "workspace_id": workspace.workspace_id,
            "workspace_name": label,
            "pane_ids": panes,
        }));
    }
    let pane_count: usize = verified
        .iter()
        .filter_map(|value| value.get("pane_ids")?.as_array().map(Vec::len))
        .sum();
    Ok(serde_json::json!({
        "ownership_verified": true,
        "target": manifest.target,
        "workspace_count": verified.len(),
        "pane_count": pane_count,
        "workspaces": verified,
    }))
}

fn cleanup(manifest: &FixtureManifest) -> Result<Value, String> {
    validate_manifest(manifest)?;
    let live = workspace_list(&manifest.target)?;
    for workspace in &manifest.workspaces {
        let label = find_workspace_label(&live, &workspace.workspace_id)
            .ok_or_else(|| format!("fixture workspace {} is absent", workspace.workspace_id))?;
        if label != workspace.workspace_name || !label.starts_with(FIXTURE_PREFIX) {
            return Err(format!(
                "cleanup refused ownership mismatch for {}",
                workspace.workspace_id
            ));
        }
    }
    for workspace in manifest.workspaces.iter().rev() {
        run_herdr_command(
            &manifest.target,
            &["workspace", "close", &workspace.workspace_id],
        )?;
    }
    let after = workspace_list(&manifest.target)?;
    if manifest
        .workspaces
        .iter()
        .any(|workspace| find_workspace_label(&after, &workspace.workspace_id).is_some())
    {
        return Err("one or more owned fixture workspaces remained after cleanup".to_owned());
    }
    cleanup_directory(&manifest.target, &manifest.cwd)?;
    Ok(serde_json::json!({
        "ownership_verified": true,
        "closed_workspace_ids": manifest.workspaces.iter().map(|workspace| &workspace.workspace_id).collect::<Vec<_>>(),
        "manifest_retained": true,
        "target": manifest.target,
    }))
}

fn prepare_directory(target: &str, cwd: &str) -> Result<(), String> {
    if target == "local" {
        fs::create_dir_all(cwd)
            .map_err(|error| format!("fixture cwd could not be created: {error}"))
    } else {
        run_command(
            Path::new("/usr/bin/ssh"),
            &["mini", "/bin/mkdir", "-p", cwd],
        )
        .map(|_| ())
    }
}

fn cleanup_directory(target: &str, cwd: &str) -> Result<(), String> {
    if !cwd.starts_with(&format!("/tmp/{FIXTURE_PREFIX}")) {
        return Err("cleanup refused a non-fixture cwd".to_owned());
    }
    if target == "local" {
        match fs::remove_dir(cwd) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "owned fixture directory could not be removed: {error}"
            )),
        }
    } else {
        run_command(Path::new("/usr/bin/ssh"), &["mini", "/bin/rmdir", cwd]).map(|_| ())
    }
}

fn workspace_list(target: &str) -> Result<Value, String> {
    run_herdr(target, &["workspace", "list"])
}

fn pane_ids(target: &str, workspace_id: &str) -> Result<Vec<String>, String> {
    let value = run_herdr(target, &["pane", "list", "--workspace", workspace_id])?;
    let mut ids = BTreeSet::new();
    collect_strings(&value, "pane_id", &mut ids);
    Ok(ids.into_iter().collect())
}

fn find_workspace_id(value: &Value, label: &str) -> Option<String> {
    value
        .get("result")?
        .get("workspaces")?
        .as_array()?
        .iter()
        .find(|workspace| workspace.get("label").and_then(Value::as_str) == Some(label))?
        .get("workspace_id")?
        .as_str()
        .map(str::to_owned)
}

fn find_workspace_label(value: &Value, workspace_id: &str) -> Option<String> {
    value
        .get("result")?
        .get("workspaces")?
        .as_array()?
        .iter()
        .find(|workspace| {
            workspace.get("workspace_id").and_then(Value::as_str) == Some(workspace_id)
        })?
        .get("label")?
        .as_str()
        .map(str::to_owned)
}

fn find_string(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => map
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| map.values().find_map(|child| find_string(child, key))),
        Value::Array(array) => array.iter().find_map(|child| find_string(child, key)),
        _ => None,
    }
}

fn collect_strings(value: &Value, key: &str, output: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(Value::as_str) {
                output.insert(found.to_owned());
            }
            for child in map.values() {
                collect_strings(child, key, output);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_strings(child, key, output);
            }
        }
        _ => {}
    }
}

fn run_herdr(target: &str, args: &[&str]) -> Result<Value, String> {
    let output = run_herdr_command(target, args)?;
    serde_json::from_slice(&output.stdout)
        .map_err(|_| "herdr returned unreadable JSON for an owned fixture operation".to_owned())
}

fn run_herdr_command(target: &str, args: &[&str]) -> Result<Output, String> {
    if target == "local" {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| "local fixture execution requires HOME".to_owned())?;
        let herdr = Path::new(&home).join(".local/bin/herdr");
        run_command(&herdr, args)
    } else {
        let mut remote = vec!["mini", REMOTE_HERDR];
        remote.extend_from_slice(args);
        run_command(Path::new("/usr/bin/ssh"), &remote)
    }
}

fn run_command(executable: &Path, args: &[&str]) -> Result<Output, String> {
    let output = Command::new(executable)
        .args(args)
        .output()
        .map_err(|error| format!("fixture command could not start: {error}"))?;
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if stderr.is_empty() {
            format!("fixture command failed with {}", output.status)
        } else {
            stderr
        })
    }
}

fn save_manifest(path: &Path, manifest: &FixtureManifest) -> Result<(), String> {
    validate_manifest(manifest)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "manifest path must have a parent directory".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("manifest directory failed: {error}"))?;
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|_| "fixture manifest could not be encoded".to_owned())?;
    let temporary = path.with_extension("json.next");
    fs::write(&temporary, bytes).map_err(|error| format!("manifest write failed: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("manifest replace failed: {error}"))
}

fn load_manifest_argument(args: &[String]) -> Result<FixtureManifest, String> {
    let path = argument_value("--manifest", args)
        .ok_or_else(|| "--manifest <path> is required".to_owned())?;
    let bytes = fs::read(&path).map_err(|error| format!("manifest read failed: {error}"))?;
    let manifest = serde_json::from_slice::<FixtureManifest>(&bytes)
        .map_err(|_| "manifest JSON does not match the fixture schema".to_owned())?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}
