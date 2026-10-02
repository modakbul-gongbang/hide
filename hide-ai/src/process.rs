//! The per-request measurement the process cap is made of: a child's
//! descendant count and its resident size, read by `hide-platform` from the
//! kernel's tables (resident-process practice, rules 4 and 5). Children are
//! started and ended through `hide_platform::process::OwnedChild`, the one
//! spawn helper, so this file only reports.

use hide_platform::process;

/// What the process cap is measured against, or why it could not be measured.
///
/// A missing measurement is `Unavailable`, never a zero: where the kernel
/// query fails the cap is not enforced, and a silent zero would read as a
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

/// Measures the process tree rooted at `pid`.
pub fn measure(pid: u32) -> ProcessMeasurement {
    match process::measure_tree(pid) {
        Ok(measured) => ProcessMeasurement::Available {
            app_server_pid: pid,
            descendants: measured.descendants,
            rss_bytes: measured.rss_bytes,
        },
        Err(_) => ProcessMeasurement::Unavailable,
    }
}
