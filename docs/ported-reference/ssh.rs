use crate::config::TargetConfig;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

pub struct TunnelManager {
    runtime_dir: PathBuf,
    children: HashMap<String, Child>,
}

impl TunnelManager {
    pub fn new(runtime_dir: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let runtime_dir = runtime_dir.into();
        fs::create_dir_all(&runtime_dir)?;
        let manager = Self {
            runtime_dir,
            children: HashMap::new(),
        };
        manager.cleanup_stale()?;
        Ok(manager)
    }

    pub fn socket_path(&self, target: &TargetConfig) -> PathBuf {
        self.runtime_dir.join(format!("{}.sock", target.id))
    }

    pub fn start(&mut self, target: &TargetConfig) -> anyhow::Result<PathBuf> {
        let Some(ssh) = &target.ssh else {
            return Ok(PathBuf::from(&target.socket_path));
        };
        let local = self.socket_path(target);
        if let Some(child) = self.children.get_mut(&target.id) {
            if child.try_wait()?.is_none() {
                return Ok(local);
            }
        }
        self.children.remove(&target.id);
        let _ = fs::remove_file(&local);
        let remote = ssh.remote_socket.replace('~', "$HOME");
        let mut command = Command::new("ssh");
        command.args(["-N", "-T", "-o", "ExitOnForwardFailure=yes"]);
        for option in &ssh.options {
            command.arg(option);
        }
        command.args(["-L", &format!("{}:{}", local.display(), remote), &ssh.host]);
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        fs::write(
            self.runtime_dir.join(format!("{}.pid", target.id)),
            child.id().to_string(),
        )?;
        self.children.insert(target.id.clone(), child);
        Ok(local)
    }

    pub fn stop_all(&mut self) {
        for (target_id, mut child) in self.children.drain() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(self.runtime_dir.join(format!("{target_id}.pid")));
            let _ = fs::remove_file(self.runtime_dir.join(format!("{target_id}.sock")));
        }
    }

    pub fn cleanup_stale(&self) -> anyhow::Result<()> {
        for entry in fs::read_dir(&self.runtime_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|v| v.to_str()) == Some("pid") {
                if let Ok(pid) = fs::read_to_string(&path) {
                    let pid = pid.trim();
                    let command = Command::new("ps")
                        .args(["-p", pid, "-o", "command="])
                        .output();
                    if let Ok(output) = command {
                        let command = String::from_utf8_lossy(&output.stdout);
                        if command.trim_start().starts_with("ssh ") {
                            let _ = Command::new("kill").arg(pid).status();
                        }
                    }
                }
                let _ = fs::remove_file(&path);
            }
            if path.extension().and_then(|v| v.to_str()) == Some("sock") {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir())
        .join("herdr-pet")
}
