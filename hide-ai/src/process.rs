//! Process ownership helpers for the crate's backends.
//!
//! Two responsibilities live here, both from `oh-my-principle`'s resident
//! process practice:
//!
//! - the single spawn helper every backend starts a child through, so
//!   ownership is one function rather than scattered `Command::spawn` calls;
//! - the per-request measurement the process cap is made of - a child's
//!   descendant count and its resident size - read from the kernel with
//!   `libproc` on macOS, and reported as `Unavailable` on every other
//!   platform rather than guessed at.

use std::io;
use std::process::{Child, Command};

/// What the process cap is measured against, or why it could not be measured.
///
/// A missing measurement is `Unavailable`, never a zero: on a platform without
/// the kernel query the cap is not enforced, and a silent zero would read as a
/// healthy tree while a leak grew underneath it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessMeasurement {
    Available {
        /// The pid of the child the crate started (the app-server wrapper).
        app_server_pid: u32,
        /// Every process under that pid, transitively; the app-server pid
        /// itself is not counted.
        descendants: usize,
        /// Resident set size summed over the app-server pid and all of its
        /// descendants.
        rss_bytes: u64,
    },
    Unavailable,
}

/// The one place a child process is started in this crate.
///
/// It exists so ownership is a single function rather than a scatter of
/// `Command::spawn` calls (resident-process practice, rule 1). On macOS there
/// is no parent-death signal; the codex app-server is instead owned through
/// the stdin pipe this helper's caller hands it, whose EOF ends the whole tree
/// when the owner dies, and the claude backend starts one child per request
/// and kills it on every path. Both routes go through here.
pub fn spawn(command: &mut Command) -> io::Result<Child> {
    command.spawn()
}

/// Measures the process tree rooted at `pid`.
///
/// macOS reads it from the kernel with `proc_listchildpids` and
/// `proc_pidinfo`, so nothing is forked to answer. Every other platform
/// answers `Unavailable`; a `/proc` reader is added when Linux is operated.
#[cfg(target_os = "macos")]
pub fn measure(pid: u32) -> ProcessMeasurement {
    let mut descendants = Vec::new();
    collect_descendants(pid as i32, &mut descendants, 0);
    let mut rss_bytes = rss_of(pid as i32);
    for child in &descendants {
        rss_bytes = rss_bytes.saturating_add(rss_of(*child));
    }
    ProcessMeasurement::Available {
        app_server_pid: pid,
        descendants: descendants.len(),
        rss_bytes,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn measure(_pid: u32) -> ProcessMeasurement {
    ProcessMeasurement::Unavailable
}

/// A depth bound stops a pid-reuse cycle from looping forever; the real tree
/// is a handful deep, so this only ever fires on a corrupt reading.
#[cfg(target_os = "macos")]
const MAX_DEPTH: u32 = 32;

#[cfg(target_os = "macos")]
fn collect_descendants(pid: i32, out: &mut Vec<i32>, depth: u32) {
    if depth >= MAX_DEPTH {
        return;
    }
    for child in child_pids(pid) {
        if child <= 0 || out.contains(&child) {
            continue;
        }
        out.push(child);
        collect_descendants(child, out, depth + 1);
    }
}

#[cfg(target_os = "macos")]
fn child_pids(pid: i32) -> Vec<i32> {
    use std::os::raw::c_void;
    // SAFETY: the first call asks for the size with a null buffer; the second
    // writes at most `capacity` ints into a buffer we own. Both are the
    // documented `proc_listchildpids` contract.
    unsafe {
        let needed = libc::proc_listchildpids(pid, std::ptr::null_mut(), 0);
        if needed <= 0 {
            return Vec::new();
        }
        // Slack for children that appear between the two calls.
        let capacity = needed as usize / std::mem::size_of::<i32>() + 16;
        let mut buffer = vec![0i32; capacity];
        let byte_len = (capacity * std::mem::size_of::<i32>()) as libc::c_int;
        let written =
            libc::proc_listchildpids(pid, buffer.as_mut_ptr() as *mut c_void, byte_len);
        if written <= 0 {
            return Vec::new();
        }
        let count = (written as usize / std::mem::size_of::<i32>()).min(capacity);
        buffer.truncate(count);
        buffer.retain(|child| *child > 0);
        buffer
    }
}

#[cfg(target_os = "macos")]
fn rss_of(pid: i32) -> u64 {
    // SAFETY: `proc_pidinfo` writes at most `size_of::<proc_taskinfo>()` bytes
    // into a value we own, and reports how many it wrote.
    unsafe {
        let mut info: libc::proc_taskinfo = std::mem::zeroed();
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        let written = libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            &mut info as *mut _ as *mut std::os::raw::c_void,
            size,
        );
        if written == size {
            info.pti_resident_size
        } else {
            0
        }
    }
}
