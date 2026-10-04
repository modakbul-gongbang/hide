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
        if line.len() > RECEIPT_BYTES {
            return Err(io::Error::other("fixture receipt byte cap exceeded"));
        }
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
            return match receipt.write("launch-failed", -1, 0, &error.to_string()) {
                Ok(()) => Err(error),
                Err(report) => Err(io::Error::new(
                    error.kind(),
                    format!("launch-failed: {error}; secondary receipt-write-failed: {report}"),
                )),
            };
        }
    };
    receipt.pid = child.id();
    receipt.birth = match start_time(child.id()) {
        Ok(birth) => birth,
        Err(error) => {
            return finish(
                &mut child,
                &receipt,
                &root,
                &request,
                root_id,
                false,
                Some(("launch-identity-failed", error)),
            );
        }
    };
    let (lost, owner) = mpsc::sync_channel(1);
    let watch = std::thread::Builder::new().spawn(move || {
        let mut byte = [0u8; 1];
        // The worker alone owns the write end; targets receive null stdin.
        let _ = lost.send(std::io::stdin().read(&mut byte));
    });
    if let Err(error) = watch {
        return finish(
            &mut child,
            &receipt,
            &root,
            &request,
            root_id,
            false,
            Some(("owner-watch-start-failed", error)),
        );
    }
    let mut phase = "launch-receipt-failed";
    let observed = (|| -> io::Result<bool> {
        receipt.write("running", 0, -1, "")?;
        loop {
            phase = "owner-pipe-failed";
            match owner.try_recv() {
                Ok(Ok(0)) => return Ok(true),
                Ok(Ok(_)) => {
                    phase = "owner-protocol-failed";
                    return Err(io::Error::other(
                        "fixture owner pipe carried unexpected bytes",
                    ));
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(io::Error::other("fixture owner watch disconnected"));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            phase = "stop-request-metadata-failed";
            match std::fs::symlink_metadata(&request) {
                Ok(metadata) => {
                    // Bound allocation even if the request grows after metadata.
                    // A directory still reaches the actual native read boundary.
                    phase = "stop-request-read-failed";
                    if metadata.is_file() && metadata.len() > 128 {
                        return Err(io::Error::other("fixture stop request byte cap exceeded"));
                    }
                    let mut intent = String::new();
                    std::fs::File::open(&request)?
                        .take(129)
                        .read_to_string(&mut intent)?;
                    if intent.len() > 128 {
                        return Err(io::Error::other("fixture stop request byte cap exceeded"));
                    }
                    if intent.trim() == format!("{} {}", receipt.pid, receipt.birth) {
                        return Ok(false);
                    }
                    phase = "stop-refusal-receipt-failed";
                    receipt.write(
                        "stop-refused",
                        -1,
                        -1,
                        "fixture launch identity mismatch; preserve home",
                    )?;
                    phase = "stop-request-remove-failed";
                    std::fs::remove_file(&request)?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            phase = "child-exit-observation-failed";
            if child.has_exited()? {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    let (owner_lost, primary) = match observed {
        Ok(owner_lost) => (owner_lost, None),
        Err(error) => (false, Some((phase, error))),
    };
    finish(
        &mut child, &receipt, &root, &request, root_id, owner_lost, primary,
    )
}

/// Every post-launch outcome attempts the same owned termination and reports
/// the original boundary plus each later failure. No failed query is an empty
/// tree, and only confirmed exit can authorize home removal.
fn finish(
    child: &mut OwnedChild,
    receipt: &Receipt,
    root: &std::path::Path,
    request: &std::path::Path,
    root_id: identity::FileId,
    owner_lost: bool,
    primary: Option<(&'static str, io::Error)>,
) -> io::Result<i32> {
    let mut failures: Vec<_> = primary.into_iter().collect();
    let (code, survivors) = match child.finish_tree_until(Instant::now() + SHUTDOWN, MEMBERS) {
        Ok(status) => (status.code().unwrap_or(-1), 0),
        Err(error) => {
            failures.push(("exit-unconfirmed", error));
            (-1, -1)
        }
    };
    if survivors == 0 {
        if let Err(error) = std::fs::remove_file(request)
            && error.kind() != io::ErrorKind::NotFound
        {
            failures.push(("stop-request-cleanup-failed", error));
        }
        if owner_lost && failures.is_empty() {
            let removal = (|| {
                if identity::file_id_nofollow(root)? != root_id {
                    return Err(io::Error::other(
                        "fixture home identity changed; preserve entry",
                    ));
                }
                std::fs::remove_dir_all(root)
            })();
            if let Err(error) = removal {
                failures.push(("home-cleanup-failed", error));
            }
        }
    }
    if let Some((phase, primary)) = failures.first() {
        // At most observation, tree, request and home failures can be present.
        // The phase remains primary even when a secondary exit is unconfirmed.
        let detail = failures
            .iter()
            .map(|(phase, error)| {
                format!(
                    "{phase}: {error}; kind={:?}; raw={:?}",
                    error.kind(),
                    error.raw_os_error()
                )
            })
            .collect::<Vec<_>>()
            .join("; secondary: ");
        let report = receipt.write(phase, -1, survivors, &detail);
        let detail = match report {
            Ok(()) => detail,
            Err(error) => format!("{detail}; receipt-write-failed: {error}"),
        };
        Err(io::Error::new(primary.kind(), detail))
    } else {
        receipt.write(
            if owner_lost {
                "owner-lost-exited"
            } else {
                "exited"
            },
            code,
            0,
            "",
        )?;
        Ok(0)
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
