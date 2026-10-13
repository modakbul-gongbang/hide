//! The existing owned SSH fixture supplies a populated remote catalog to the
//! same page/CPU/WS measurement tools used for local terminal responsiveness.
use super::*;

fn fingerprint(environment: &Environment, path: &Path) -> Result<Value> {
    let mut hash = environment.command("/usr/bin/shasum");
    hash.args(["-a", "256"]).arg(path);
    let hash = String::from_utf8(successful(hash)?)?;
    Ok(json!({"path":path,"sha256":hash.split_whitespace().next().context("sha256 output")?}))
}

pub(super) fn prepare_account(environment: &Environment) -> Result<()> {
    let project = environment.home.join("project");
    let mut git = environment.command("/usr/bin/git");
    git.arg("-C").arg(&project).args([
        "-c",
        "user.name=fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "measurement fixture",
    ]);
    successful(git)?;
    for n in 0..40 {
        let mut git = environment.command("/usr/bin/git");
        git.arg("-C")
            .arg(&project)
            .args(["branch", &format!("feature-{n}")]);
        successful(git)?;
    }
    let mut git = environment.command("/usr/bin/git");
    git.arg("-C")
        .arg(&project)
        .args(["worktree", "add", "-q", "-b", "checkout-feature"])
        .arg(environment.home.join("project-worktree"));
    successful(git)?;
    let mut origin = environment.command("/usr/bin/git");
    origin.arg("-C").arg(&project).args([
        "remote",
        "add",
        "origin",
        "https://github.com/fixture/project.git",
    ]);
    successful(origin)?;
    let script = r#"#!/usr/bin/python3
import json, subprocess, sys
a = sys.argv[1:]
if a[:2] == ['auth', 'status']: sys.exit(0)
if a[:2] == ['repo', 'view']:
 print(json.dumps({'nameWithOwner': 'fixture/project'})); sys.exit(0)
if a[:2] == ['pr', 'list']:
 oid = subprocess.check_output(['/usr/bin/git', 'rev-parse', 'HEAD'], text=True).strip()
 print(json.dumps([{'number': n+1, 'title': 'Fixture pull request '+str(n), 'headRefName': 'feature-'+str(n), 'headRefOid': oid, 'baseRefName': 'main', 'state': 'OPEN', 'reviewDecision': 'REVIEW_REQUIRED', 'isDraft': False, 'isCrossRepository': False, 'url': 'https://github.com/fixture/project/pull/'+str(n+1), 'mergedAt': None, 'updatedAt': '2026-01-01T00:00:00Z', 'statusCheckRollup': [{'__typename':'CheckRun','name':'verify','status':'COMPLETED','conclusion':'SUCCESS'}], 'closingIssuesReferences': []} for n in range(40)])); sys.exit(0)
if a[:2] == ['issue', 'list']: print('[]'); sys.exit(0)
sys.stderr.write('measurement fixture refuses unsupported GitHub command\n'); sys.exit(1)
"#;
    for directory in [
        environment.home.join("bin"),
        environment.home.join(".local/bin"),
    ] {
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("gh"), script)?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory.join("gh"), fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn measure(fixture: &mut Fixture) -> Result<()> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let label =
        std::env::var("RESPONSIVE_MEASURE_LABEL").context("set RESPONSIVE_MEASURE_LABEL")?;
    ensure!(
        !label.is_empty()
            && label.len() <= 80
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "invalid measurement label"
    );
    let scale = std::env::var("RESPONSIVE_MEASURE_SCALE").unwrap_or_else(|_| "small".into());
    ensure!(
        matches!(scale.as_str(), "small" | "operator"),
        "invalid measurement scale"
    );
    let output = repository
        .join("agents/runs/responsive-state/measure")
        .join(&label);
    ensure!(!output.exists(), "measurement output already exists");
    fs::create_dir_all(output.join("chrome-profile"))?;
    let mut source = fixture.local.environment.command("/usr/bin/git");
    source
        .arg("-C")
        .arg(fixture.source_cli.parent().unwrap().parent().unwrap())
        .args(["rev-parse", "HEAD"]);
    let binary_head = String::from_utf8(successful(source)?)?.trim().to_owned();
    ensure!(
        binary_head
            == std::env::var("RESPONSIVE_BINARY_HEAD").context("set RESPONSIVE_BINARY_HEAD")?,
        "binary worktree head mismatch"
    );
    let mut inputs = Vec::new();
    for name in ["hide", "hided", "hide-agent-hooks"] {
        inputs.push(json!({"name":name,"source":fingerprint(&fixture.local.environment, &fixture.source_cli.join(name))?,"executed":fingerprint(&fixture.local.environment, &fixture.hide.parent().unwrap().join(name))?}));
    }
    let source_web = fixture
        .source_cli
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("web/dist");
    let mut assets = vec![source_web.join("index.html")];
    for entry in fs::read_dir(source_web.join("assets"))? {
        let path = entry?.path();
        if path.is_file() {
            assets.push(path);
        }
    }
    assets.sort();
    let mut web_inputs = Vec::new();
    for asset in assets {
        web_inputs.push(fingerprint(&fixture.local.environment, &asset)?);
    }
    fs::write(
        output.join("web-assets.json"),
        serde_json::to_vec_pretty(&web_inputs)?,
    )?;
    let mut driver_inputs = Vec::new();
    for name in [
        "responsive.mjs",
        "cdp.mjs",
        "resources.py",
        "topology.mjs",
        "key-echo.mjs",
        "key-count.mjs",
        "ws-count.mjs",
        "scale.sh",
        "core-cost.rs",
        "build-core-cost.py",
    ] {
        driver_inputs.push(fingerprint(
            &fixture.local.environment,
            &repository.join("scripts/web-shell-measure").join(name),
        )?);
    }
    for name in [
        "hided/tests/it/responsive_state.rs",
        "hided/tests/it/support/remote_delivery/responsive.rs",
        "hided/tests/it/support/remote_delivery/mod.rs",
    ] {
        driver_inputs.push(fingerprint(
            &fixture.local.environment,
            &repository.join(name),
        )?);
    }
    let mut harness_head = fixture.local.environment.command("/usr/bin/git");
    harness_head
        .arg("-C")
        .arg(repository)
        .args(["rev-parse", "HEAD"]);
    let harness_head = String::from_utf8(successful(harness_head)?)?
        .trim()
        .to_owned();
    let mut version = fixture
        .local
        .environment
        .command(&fixture.local.environment.values[OsStr::new("HERDR_BIN_PATH")]);
    version.arg("--version");
    let herdr_version = String::from_utf8(successful(version)?)?.trim().to_owned();
    let layout = fixture.local.run(&["api", "snapshot"])?;
    let root = layout
        .pointer("/result/snapshot")
        .or_else(|| layout.get("result"))
        .context("Herdr snapshot result")?;
    let pane = root["panes"]
        .as_array()
        .context("Herdr panes")?
        .iter()
        .find(|pane| pane["pane_id"] == fixture.local.pane)
        .context("fixture pane")?;
    let tab = pane["tab_id"].as_str().context("fixture tab")?.to_owned();
    let workspace = pane["workspace_id"]
        .as_str()
        .context("fixture workspace")?
        .to_owned();
    fixture
        .local
        .write(&["pane", "send-keys", &fixture.local.pane, "ctrl+c"])?;
    wait_for("measurement shell prompt", || {
        Ok(fixture.local.text()?.contains(PROMPT).then_some(()))
    })?;
    fixture.local.write(&[
        "pane",
        "run",
        &fixture.local.pane,
        "stty -echo -icanon; cat",
    ])?;
    let mut other = Vec::new();
    let mut previous = fixture.local.pane.clone();
    for direction in ["right", "down", "right", "down"] {
        let created = fixture.local.run(&[
            "pane",
            "split",
            &previous,
            "--direction",
            direction,
            "--no-focus",
        ])?;
        previous = created["result"]["pane"]["pane_id"]
            .as_str()
            .context("split pane")?
            .into();
        other.push(previous.clone());
    }
    fixture.local.run(&[
        "tab",
        "create",
        "--workspace",
        &workspace,
        "--label",
        "switch",
        "--no-focus",
    ])?;
    if scale == "operator" {
        let mut command = fixture.local.environment.command("/bin/bash");
        command
            .arg(repository.join("scripts/web-shell-measure/scale.sh"))
            .args(["build", "operator"])
            .env("MEASURE_RUN_DIR", &output)
            .env("MEASURE_SOCKET_DIR", fixture.ipc.path())
            .env("MEASURE_PREEXISTING_SPLITS", "4")
            .env("MEASURE_RICH_CATALOG", "1")
            .stdin(Stdio::null())
            .stdout(File::create(output.join("scale-build.log"))?)
            .stderr(File::create(output.join("scale-build.stderr.log"))?);
        let mut child = OwnedChild::spawn(&mut command)?;
        let status = wait_within("operator fixture build", Duration::from_secs(180), || {
            Ok(child.try_wait()?)
        })?;
        ensure!(status.success(), "operator fixture build failed: {status}");
    }
    let status_pane = if scale == "operator" {
        let printing = fs::read_to_string(output.join("scale-printing.txt"))?;
        let pane = printing
            .lines()
            .next()
            .context("operator printer")?
            .to_owned();
        other.push(pane.clone());
        pane
    } else {
        fixture.local.pane.clone()
    };
    let window = renderer::Renderer::window(fixture.port, &fixture.token)?;
    let snapshot = wait_within(
        "populated local and remote catalogs",
        Duration::from_secs(180),
        || {
            let snapshot = fixture.snapshot()?;
            let local = snapshot
                .pointer("/navigator/workspaces")
                .and_then(Value::as_array)
                .context("local workspaces")?;
            let remote = snapshot
                .pointer("/status/remote/0/session/workspaces")
                .and_then(Value::as_array);
            Ok((local.len() >= if scale == "operator" { 43 } else { 1 }
                && remote.is_some_and(|rows| !rows.is_empty()))
            .then_some(snapshot))
        },
    )?;
    for workspace in snapshot["navigator"]["workspaces"]
        .as_array()
        .context("workspace inventory")?
    {
        fixture.event(
            "workspace_pin_set",
            json!({"workspace_id":workspace["id"],"pinned":true}),
        )?;
    }
    let snapshot = wait_within(
        "rich branch and pull request catalogs",
        Duration::from_secs(180),
        || {
            let snapshot = fixture.snapshot()?;
            let rows = snapshot["navigator"]["workspaces"]
                .as_array()
                .context("rich workspace inventory")?;
            Ok(rows
                .iter()
                .all(|row| {
                    row["branches"].as_array().is_some_and(|b| b.len() >= 40)
                        && row["pull_requests"]
                            .as_array()
                            .is_some_and(|p| p.len() >= 40)
                })
                .then_some(snapshot))
        },
    )?;
    drop(window);
    let selected_project = snapshot["navigator"]["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| {
            w["checkouts"].as_array().is_some_and(|rows| {
                rows.iter().any(|c| {
                    c["path"]
                        == fixture
                            .local
                            .environment
                            .home
                            .join("project")
                            .to_string_lossy()
                            .as_ref()
                })
            })
        })
        .context("measured project")?;
    let project_id = selected_project["id"].as_str().context("project id")?;
    let checkout = selected_project["checkouts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["path"]
                == fixture
                    .local
                    .environment
                    .home
                    .join("project")
                    .to_string_lossy()
                    .as_ref()
        })
        .context("measured checkout")?["id"]
        .as_str()
        .context("checkout id")?
        .to_owned();
    let actual_topology = fixture.local.run(&["api", "snapshot"])?;
    let topology = actual_topology
        .pointer("/result/snapshot")
        .or_else(|| actual_topology.get("result"))
        .context("measured Herdr topology")?;
    let topology_counts = json!({
        "workspaces": topology["workspaces"].as_array().context("measured workspaces")?.len(),
        "tabs": topology["tabs"].as_array().context("measured tabs")?.len(),
        "panes": topology["panes"].as_array().context("measured panes")?.len(),
        "configured_agent_targets": if scale == "operator" { 30 } else { 0 },
    });
    if scale == "operator" {
        ensure!(
            topology_counts["workspaces"] == 43
                && topology_counts["tabs"] == 62
                && topology_counts["panes"] == 66,
            "operator topology did not converge: {topology_counts}"
        );
    }
    let helper = fixture
        .remote
        .environment
        .home
        .join("helper/current/hided")
        .canonicalize()?;
    let mut helper_version = fixture.remote.environment.command(&helper);
    helper_version.args(["node", "--version"]);
    let helper_version = String::from_utf8(successful(helper_version)?)?
        .trim()
        .to_owned();
    let mut remote_version = fixture
        .remote
        .environment
        .command(&fixture.remote.environment.values[OsStr::new("HERDR_BIN_PATH")]);
    remote_version.arg("--version");
    let remote_version = String::from_utf8(successful(remote_version)?)?
        .trim()
        .to_owned();
    let remote_identity = json!({
        "helper": fingerprint(&fixture.remote.environment, &helper)?,
        "helper_version": helper_version,
        "herdr_version": remote_version,
        "herdr": fingerprint(&fixture.remote.environment, Path::new(&fixture.remote.environment.values[OsStr::new("HERDR_BIN_PATH")]))?,
        "herdr_pid": fixture.remote.server.as_ref().unwrap().id(),
        "host": snapshot["navigator"]["devices"].as_array().and_then(|rows| rows.iter().find(|row| row["id"] == "remote")).and_then(|row| row.get("host")),
    });
    let chrome_bin = std::env::var_os("RESPONSIVE_CHROME_BIN")
        .context("set RESPONSIVE_CHROME_BIN to the fixed measurement browser")?;
    ensure!(
        Path::new(&chrome_bin).is_file(),
        "measurement browser missing"
    );
    let mut version = fixture.local.environment.command(&chrome_bin);
    version.arg("--version");
    let chrome_version = String::from_utf8(successful(version)?)?.trim().to_owned();
    let chrome_identity = fingerprint(&fixture.local.environment, Path::new(&chrome_bin))?;
    let node_bin = std::env::var_os("RESPONSIVE_NODE_BIN").context("set RESPONSIVE_NODE_BIN")?;
    let mut version = fixture.local.environment.command(&node_bin);
    version.arg("--version");
    let node_version = String::from_utf8(successful(version)?)?.trim().to_owned();
    let node_identity = fingerprint(&fixture.local.environment, Path::new(&node_bin))?;
    let mut chrome = fixture.local.environment.command(&chrome_bin);
    chrome
        .args([
            "--headless=new",
            "--force-device-scale-factor=1",
            "--remote-debugging-port=0",
            "--remote-debugging-address=127.0.0.1",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-sync",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-background-timer-throttling",
            "--disable-renderer-backgrounding",
            "--disable-backgrounding-occluded-windows",
            "--window-size=1280,900",
        ])
        .arg(format!(
            "--user-data-dir={}",
            output.join("chrome-profile").display()
        ))
        .arg("about:blank")
        .stdout(File::create(output.join("chrome.log"))?)
        .stderr(File::create(output.join("chrome.stderr.log"))?);
    let chrome = OwnedChild::spawn(&mut chrome)?;
    let cdp = wait_for("owned Chrome debugger", || {
        Ok(
            fs::read_to_string(output.join("chrome-profile/DevToolsActivePort"))
                .ok()
                .and_then(|s| s.lines().next().and_then(|s| s.parse::<u16>().ok())),
        )
    })?;
    let identity = json!({"label":label,"scale":scale,"binary_head":binary_head,"binaries":inputs,"harness_head":harness_head,"harness":driver_inputs,"herdr_version":herdr_version,"herdr":fingerprint(&fixture.local.environment, Path::new(&fixture.local.environment.values[OsStr::new("HERDR_BIN_PATH")]))?,"remote":remote_identity,"topology":topology_counts,"chrome":chrome_identity,"chrome_version":chrome_version,"node":node_identity,"node_version":node_version,"hided_pid":fixture.daemon.as_ref().unwrap().id(),"herdr_pid":fixture.local.server.as_ref().unwrap().id(),"chrome_pid":chrome.id(),"viewport":[1280,900],"dpr":1,"pane_count":5,"remote_sessions":1,"build":"release","client_count_during_measurement":1,"printing_panes":other.len(),"other_panes":other,"status_pane":status_pane,"pane":fixture.local.pane,"tab":tab});
    fs::write(
        output.join("identity.json"),
        serde_json::to_vec_pretty(&identity)?,
    )?;
    let config = json!({"url":format!("http://127.0.0.1:{}/?probe=1#token={}",fixture.port,fixture.token),"workspace":project_id,"checkout":checkout,"cdp":cdp,"output":output,"identity":identity});
    let config_path = output.join("private-config.json");
    hide_platform::fs::atomic::write_file(
        &config_path,
        &serde_json::to_vec(&config)?,
        hide_platform::fs::Access::Private,
    )?;
    let node = std::env::var_os("RESPONSIVE_NODE_BIN").context("set RESPONSIVE_NODE_BIN")?;
    let mut command = fixture.local.environment.command(node);
    command
        .arg(repository.join("scripts/web-shell-measure/responsive.mjs"))
        .arg(&config_path)
        .stdin(Stdio::null())
        .stdout(File::create(output.join("driver.log"))?)
        .stderr(File::create(output.join("driver.stderr.log"))?);
    // capture() is a ten-second command probe and replaces redirected output.
    // This owned, bounded journey keeps its concurrent CPU/latency drivers alive.
    let result = (|| -> Result<()> {
        let mut driver = OwnedChild::spawn(&mut command)?;
        let status = wait_within(
            "private measurement driver",
            Duration::from_secs(600),
            || Ok(driver.try_wait()?),
        )?;
        ensure!(
            status.success(),
            "private measurement driver failed: {status}; see driver.stderr.log"
        );
        Ok(())
    })();
    fs::remove_file(config_path)?;
    drop(chrome);
    result?;
    fs::copy(fixture.root.join("hided.log"), output.join("hided.log"))?;
    fs::copy(
        fixture.state.join("Logs/core.jsonl"),
        output.join("core.jsonl"),
    )?;
    if let Some(mut daemon) = fixture.daemon.take() {
        daemon.kill_tree()?;
        wait_for("measured daemon exit before core probe", || {
            Ok(daemon.try_wait()?.map(|_| ()))
        })?;
    }
    // A separate observation-only core uses the same real fixture after the
    // product-page windows. Its timings must not be merged into DOM latency.
    let probe = fixture.source_cli.join("responsive-core-cost");
    ensure!(probe.is_file(), "build the release core-cost probe first");
    fs::copy(
        fixture
            .source_cli
            .join("responsive-core-cost.identity.json"),
        output.join("core-cost-build.json"),
    )?;
    let cost_state = output.join("cost-state");
    fs::create_dir_all(&cost_state)?;
    fs::copy(
        fixture.state.join("core-state.json"),
        cost_state.join("core-state.json"),
    )?;
    fs::copy(
        fixture.state.join("workspace-views.json"),
        cost_state.join("workspace-views.json"),
    )?;
    let node: Value = serde_json::from_slice(&fs::read(fixture.state.join("node.json"))?)?;
    let cost_config = json!({
        "output":output, "pane":fixture.local.pane, "herdr_workspace":workspace,
        "workspaces":if scale == "operator" {43} else {1},
        "options":{
            "schema_version":2, "node_id":node["node"],
            "home":fixture.local.environment.home,
            "app_state_path":cost_state.join("core-state.json"),
            "workspace_views_path":cost_state.join("workspace-views.json"),
            "herdr_socket_path":fixture.ipc.path().join("l.sock"),
            "herdr_bin_path":fixture.local.environment.values[OsStr::new("HERDR_BIN_PATH")].to_string_lossy(),
            "host_helper_root":fixture.remote.environment.home.join("helper"),
            "host_cli_dir":fixture.remote.environment.home.join("bin"),
        }
    });
    let cost_config_path = output.join("cost-config.json");
    hide_platform::fs::atomic::write_file(
        &cost_config_path,
        &serde_json::to_vec(&cost_config)?,
        hide_platform::fs::Access::Private,
    )?;
    fs::write(
        output.join("core-cost-binary.json"),
        serde_json::to_vec_pretty(&fingerprint(&fixture.local.environment, &probe)?)?,
    )?;
    let mut command = fixture.local.environment.command(probe);
    command
        .arg(&cost_config_path)
        .stdin(Stdio::null())
        .stdout(File::create(output.join("core-cost.log"))?)
        .stderr(File::create(output.join("core-cost.stderr.log"))?);
    let cost_result = (|| -> Result<()> {
        let mut child = OwnedChild::spawn(&mut command)?;
        let status = wait_within(
            "release core boundary probe",
            Duration::from_secs(240),
            || Ok(child.try_wait()?),
        )?;
        ensure!(status.success(), "core boundary probe failed: {status}");
        Ok(())
    })();
    fs::remove_file(cost_config_path)?;
    cost_result?;
    Ok(())
}
