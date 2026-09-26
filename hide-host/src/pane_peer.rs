//! OS process identity used by the pane bootstrap on either side of SSH.

use std::os::fd::AsRawFd;

const MAX_PARENT_HOPS: usize = 32;

#[cfg(target_os = "macos")]
pub fn peer_pid(stream: &impl AsRawFd) -> Option<i32> {
    let mut pid: libc::pid_t = 0;
    let mut size = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: the output pointers refer to initialized stack values and the
    // fd remains owned by the stream for this call.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut size,
        )
    };
    (result == 0 && size as usize == std::mem::size_of::<libc::pid_t>() && pid > 0).then_some(pid)
}

#[cfg(target_os = "linux")]
pub fn peer_pid(stream: &impl AsRawFd) -> Option<i32> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the output pointers refer to writable values of their declared sizes.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut size,
        )
    };
    (result == 0 && size as usize == std::mem::size_of::<libc::ucred>() && credentials.pid > 0)
        .then_some(credentials.pid)
}

pub fn descends_from(mut pid: i32, shell_pid: i32) -> bool {
    for _ in 0..MAX_PARENT_HOPS {
        if pid == shell_pid {
            return true;
        }
        if pid <= 1 {
            return false;
        }
        let Some(parent) = parent_pid(pid) else {
            return false;
        };
        if parent == pid {
            return false;
        }
        pid = parent;
    }
    false
}

#[cfg(target_os = "macos")]
fn process_info(pid: i32) -> Option<libc::proc_bsdinfo> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    // SAFETY: the buffer is valid and read only after a complete result.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            std::mem::size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    (written as usize == std::mem::size_of::<libc::proc_bsdinfo>()).then_some(info)
}

#[cfg(target_os = "macos")]
fn parent_pid(pid: i32) -> Option<i32> {
    process_info(pid).map(|info| info.pbi_ppid as i32)
}

#[cfg(target_os = "macos")]
pub fn process_start(pid: i32) -> Option<u64> {
    process_info(pid).map(|info| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

#[cfg(target_os = "linux")]
fn proc_fields(pid: i32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()?
        .rsplit_once(") ")
        .map(|(_, fields)| fields.to_owned())
}

#[cfg(target_os = "linux")]
fn parent_pid(pid: i32) -> Option<i32> {
    proc_fields(pid)?.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(target_os = "linux")]
pub fn process_start(pid: i32) -> Option<u64> {
    proc_fields(pid)?.split_whitespace().nth(19)?.parse().ok()
}
