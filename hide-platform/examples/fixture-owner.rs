//! Test-only supervisor of a fixture's native process namespace.
//! stdin is a dedicated owner pipe; EOF means the worker itself has ended.
use hide_platform::fs::{Access, atomic, identity};
use hide_platform::process::{OwnedChild, start_time};
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MEMBERS: usize = 256;
const RECEIPT_BYTES: usize = 16 * 1024;
const SHUTDOWN: Duration = Duration::from_secs(2);

struct Receipt {
    file: PathBuf,
    pid: u32,
    birth: u64,
}
impl Receipt {
    fn write(&self, phase: &str, code: i32, survivors: i32, error: &str) -> io::Result<()> {
        // Fixed versioned fields. Hex encodes native error detail without
        // ambiguous lines; neither argv nor environment values enter it.
        if error.len() > RECEIPT_BYTES / 2 {
            return Err(io::Error::other("fixture error receipt byte cap exceeded"));
        }
        let detail: String = error
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let line = format!(
            "v1\t{phase}\t{}\t{}\t{code}\t{survivors}\t{detail}\n",
            self.pid, self.birth
        );
        atomic::write_file(&self.file, line.as_bytes(), Access::Private)?;
        Ok(())
    }
}

fn run() -> io::Result<i32> {
    let mut args = std::env::args_os().skip(1);
    let file = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::other("fixture receipt path missing"))?,
    );
    let root = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::other("fixture home missing"))?,
    );
    let executable = args
        .next()
        .ok_or_else(|| io::Error::other("fixture executable missing"))?;
    if !file.is_absolute()
        || !root.is_absolute()
        || !root.is_dir()
        || file.starts_with(&root)
        || std::fs::symlink_metadata(&root)?.file_type().is_symlink()
    {
        return Err(io::Error::other(
            "fixture receipt must be outside its absolute owned home",
        ));
    }
    match std::fs::symlink_metadata(&file) {
        Ok(_) => {
            return Err(io::Error::other(
                "fixture receipt belongs to an existing owner",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let root_id = identity::file_id_nofollow(&root)?;
    let request = file.with_extension("stop");
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut receipt = Receipt {
        file,
        pid: 0,
        birth: 0,
    };
    let mut child = match OwnedChild::spawn(&mut command) {
        Ok(child) => child,
        Err(error) => {
            receipt.write("launch-failed", -1, 0, &error.to_string())?;
            return Err(error);
        }
    };
    receipt.pid = child.id();
    receipt.birth = match start_time(child.id()) {
        Ok(birth) => birth,
        Err(error) => {
            let cleanup = child.finish_tree_until(Instant::now() + SHUTDOWN, MEMBERS);
            receipt.write(
                "launch-identity-failed",
                -1,
                if cleanup.is_err() { -1 } else { 0 },
                &format!("{error}; cleanup: {cleanup:?}"),
            )?;
            return Err(error);
        }
    };
    // -1 is unobserved, never a fabricated survivor count.
    receipt.write("running", 0, -1, "")?;
    let (lost, owner) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut byte = [0u8; 1];
        // No target inherits this pipe. Only worker death or explicit pipe
        // closure can end the lifetime watch, even after root exit.
        let answer = std::io::stdin().read(&mut byte);
        let _ = lost.send(answer);
    });
    let mut primary = None;
    let owner_lost = loop {
        match owner.try_recv() {
            Ok(Ok(0)) => break true,
            Ok(Ok(_)) => {
                primary = Some((
                    "owner-protocol-failed",
                    io::Error::other("fixture owner pipe carried unexpected bytes"),
                ));
                break true;
            }
            Ok(Err(error)) => {
                primary = Some(("owner-pipe-failed", error));
                break true;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                primary = Some((
                    "owner-pipe-failed",
                    io::Error::other("fixture owner watch disconnected"),
                ));
                break true;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if request.exists() {
            if std::fs::metadata(&request)?.len() > 128 {
                receipt.write(
                    "stop-refused",
                    -1,
                    -1,
                    "fixture stop request byte cap exceeded",
                )?;
            } else {
                let intent = std::fs::read_to_string(&request)?;
                let expected = format!("{} {}", receipt.pid, receipt.birth);
                if intent.trim() != expected {
                    receipt.write(
                        "stop-refused",
                        -1,
                        -1,
                        "fixture launch identity mismatch; preserve home",
                    )?;
                } else {
                    break false;
                }
            }
            std::fs::remove_file(&request)?;
        }
        if child.has_exited()? {
            break false;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let deadline = Instant::now() + SHUTDOWN;
    let result = child.finish_tree_until(deadline, MEMBERS);
    match result {
        Ok(status) => {
            // Normal stop leaves deletion to the calling fixture. Hard owner
            // loss deletes only this supervisor's root after confirmed exit.
            if owner_lost {
                let removal = (|| {
                    if identity::file_id_nofollow(&root)? != root_id {
                        return Err(io::Error::other(
                            "fixture home identity changed; preserve entry",
                        ));
                    }
                    std::fs::remove_dir_all(&root)
                })();
                if let Err(error) = removal {
                    let detail = primary
                        .as_ref()
                        .map(|(_, source)| format!("{source}; home cleanup also failed: {error}"))
                        .unwrap_or_else(|| error.to_string());
                    receipt.write("home-cleanup-failed", -1, 0, &detail)?;
                    return Err(io::Error::new(error.kind(), detail));
                }
            }
            if request.exists() {
                std::fs::remove_file(request)?;
            }
            if let Some((phase, error)) = primary {
                receipt.write(phase, -1, 0, &error.to_string())?;
                Err(error)
            } else {
                receipt.write(
                    if owner_lost {
                        "owner-lost-exited"
                    } else {
                        "exited"
                    },
                    status.code().unwrap_or(-1),
                    0,
                    "",
                )?;
                Ok(0)
            }
        }
        Err(error) => {
            let detail = primary
                .as_ref()
                .map(|(_, source)| format!("{source}; owned exit also failed: {error}"))
                .unwrap_or_else(|| error.to_string());
            receipt.write("exit-unconfirmed", -1, -1, &detail)?;
            Err(io::Error::new(error.kind(), detail))
        }
    }
}
fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("fixture.owner_failed {error}");
            std::process::exit(1)
        }
    }
}
