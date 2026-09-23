//! One owned, bounded path for launching the host's file association handler.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{Notify, Semaphore};

#[cfg(unix)]
use crate::spawn::{handoff_default_opener, spawn_opener};

const MAX_IN_FLIGHT_OPENERS: usize = 4;
const MAX_OPENS_PER_MINUTE: usize = 12;
const OPENER_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct OpenHandler {
    configured: Option<PathBuf>,
    slots: Arc<Semaphore>,
    recent: Arc<Mutex<VecDeque<Instant>>>,
    shutdown: Arc<Notify>,
}

impl OpenHandler {
    pub fn new(configured: Option<PathBuf>, shutdown: Arc<Notify>) -> Self {
        Self {
            configured,
            slots: Arc::new(Semaphore::new(MAX_IN_FLIGHT_OPENERS)),
            recent: Arc::new(Mutex::new(VecDeque::new())),
            shutdown,
        }
    }

    pub fn in_flight(&self) -> usize {
        MAX_IN_FLIGHT_OPENERS - self.slots.available_permits()
    }

    /// A successful call means the handler accepted the file, not that the
    /// eventual application opened it. An explicit CLI helper is ended within
    /// ten seconds or on daemon stop; the OS default is handed off at spawn.
    pub fn launch(&self, path: &Path) -> Result<(), &'static str> {
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| "over_budget")?;
        {
            let now = Instant::now();
            let mut recent = self.recent.lock().map_err(|_| "over_budget")?;
            while recent
                .front()
                .is_some_and(|at| now.duration_since(*at) >= Duration::from_secs(60))
            {
                recent.pop_front();
            }
            if recent.len() >= MAX_OPENS_PER_MINUTE {
                return Err("over_budget");
            }
            recent.push_back(now);
        }

        #[cfg(windows)]
        if self.configured.is_none() {
            // ShellExecuteW passes a pathname directly to the association API.
            // `cmd /C start` would interpret metacharacters in that pathname.
            let result = shell_open(path);
            drop(permit);
            return result;
        }

        #[cfg(unix)]
        if self.configured.is_none() {
            // Starting the OS association utility is the handoff. It may
            // become or wait for the chosen application, so it is not an
            // owned CLI helper and must not enter the ten-second kill path.
            let result =
                handoff_default_opener(platform_opener(), path).map_err(|_| "spawn_failed");
            drop(permit);
            return result;
        }

        #[cfg(unix)]
        let mut child = {
            let program = self.configured.as_deref().expect("configured opener");
            let supervisor = std::env::current_exe().map_err(|_| "spawn_failed")?;
            spawn_opener(&supervisor, program.as_os_str(), path).map_err(|_| "spawn_failed")?
        };
        #[cfg(windows)]
        return Err("spawn_failed");
        #[cfg(unix)]
        {
            let shutdown = Arc::clone(&self.shutdown);
            tokio::spawn(async move {
                let deadline = tokio::time::sleep(OPENER_TIMEOUT);
                tokio::pin!(deadline);
                loop {
                    tokio::select! {
                        _ = &mut deadline => break,
                        _ = shutdown.notified() => break,
                        _ = tokio::time::sleep(Duration::from_millis(50)) => {
                            match child.try_wait() {
                                Ok(true) => break,
                                Ok(false) => {},
                                Err(_) => break,
                            }
                        },
                    }
                }
                child.stop();
                drop(permit);
            });
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
fn platform_opener() -> &'static std::ffi::OsStr {
    std::ffi::OsStr::new("open")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_opener() -> &'static std::ffi::OsStr {
    std::ffi::OsStr::new("xdg-open")
}

#[cfg(windows)]
fn shell_open(path: &Path) -> Result<(), &'static str> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: isize,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }
    let verb = "open\0".encode_utf16().collect::<Vec<_>>();
    let file = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // A ShellExecuteW return above 32 means the association accepted it.
    let result = unsafe {
        ShellExecuteW(
            0,
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result <= 32 {
        Err("spawn_failed")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_handler_is_shell_free() {
        #[cfg(target_os = "macos")]
        assert_eq!(platform_opener(), "open");
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(platform_opener(), "xdg-open");
    }

    #[test]
    fn a_burst_is_bounded() {
        let handler = OpenHandler::new(None, Arc::new(Notify::new()));
        let mut recent = handler.recent.lock().unwrap();
        for _ in 0..MAX_OPENS_PER_MINUTE {
            recent.push_back(Instant::now());
        }
        drop(recent);
        assert_eq!(handler.launch(Path::new("/ignored")), Err("over_budget"));
    }
}
