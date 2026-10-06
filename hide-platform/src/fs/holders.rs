//! Which processes hold a folder open, read from the system's handle table.
//!
//! A rename or a delete of a folder fails on Windows while any process holds a
//! handle opened without delete sharing on it or on something inside it, and
//! the error (32 or 5) names no one. This asks who, so the failure's record can
//! say whether the holder is this process or another one. The Restart Manager
//! answers for files only, and the holder of a folder is usually a folder
//! handle, so the table of every open handle is read instead, and each file
//! handle's path is asked of a duplicate of it. Only Windows has the question;
//! elsewhere the module is empty.

#[cfg(windows)]
pub use imp::{Holder, holders_of};

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::path::Path;

    use windows_sys::Wdk::System::SystemInformation::NtQuerySystemInformation;
    use windows_sys::Win32::Foundation::{
        CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, FILE_TYPE_DISK, GetFileType, GetFinalPathNameByHandleW,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };

    /// `SystemExtendedHandleInformation`.
    const EXTENDED_HANDLE_INFORMATION: i32 = 64;
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
    /// The table is read into a buffer that grows until it fits, to this size.
    const TABLE_LIMIT: usize = 256 << 20;

    /// Access masks a synchronous named pipe's handle carries. Asking such a
    /// handle anything can block forever, and no folder is held through one.
    const PIPE_ACCESS: [u32; 3] = [0x0012_019f, 0x001a_019f, 0x0012_0189];

    #[repr(C)]
    struct TableHeader {
        count: usize,
        reserved: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TableEntry {
        object: *mut c_void,
        pid: usize,
        handle: usize,
        access: u32,
        backtrace: u16,
        type_index: u16,
        attributes: u32,
        reserved: u32,
    }

    /// One open handle on the folder or on something inside it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Holder {
        pub pid: u32,
        /// The executable's file name, or `?` when the process cannot be asked.
        pub process: String,
        /// What the handle is open on.
        pub path: String,
        /// The access the handle was opened with.
        pub access: u32,
    }

    /// The handles open on `folder` or on a path inside it, in every process
    /// this one may duplicate a handle from, at most `limit` of them.
    pub fn holders_of(folder: &Path, limit: usize) -> std::io::Result<Vec<Holder>> {
        let wanted = normalized(&std::fs::canonicalize(folder)?.display().to_string());
        let table = read_table()?;
        let header = table.as_ptr().cast::<TableHeader>();
        // SAFETY: the table starts with the header the call wrote.
        let count = unsafe { (*header).count };
        let entries = unsafe {
            std::slice::from_raw_parts(
                table
                    .as_ptr()
                    .add(std::mem::size_of::<TableHeader>())
                    .cast::<TableEntry>(),
                count.min(
                    (table.len() - std::mem::size_of::<TableHeader>())
                        / std::mem::size_of::<TableEntry>(),
                ),
            )
        };
        let mut owners: HashMap<usize, Option<HANDLE>> = HashMap::new();
        let mut found = Vec::new();
        for entry in entries {
            if found.len() >= limit {
                break;
            }
            if PIPE_ACCESS.contains(&entry.access) {
                continue;
            }
            let owner = *owners.entry(entry.pid).or_insert_with(|| {
                // SAFETY: a plain process open; a null result means no access.
                let process = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, entry.pid as u32) };
                (!process.is_null()).then_some(process)
            });
            let Some(owner) = owner else { continue };
            let Some(path) = path_of(owner, entry.handle) else {
                continue;
            };
            if !normalized(&path).starts_with(&wanted) {
                continue;
            }
            found.push(Holder {
                pid: entry.pid as u32,
                process: process_name(entry.pid as u32),
                path,
                access: entry.access,
            });
        }
        for owner in owners.into_values().flatten() {
            // SAFETY: each was opened above and is closed once.
            unsafe { CloseHandle(owner) };
        }
        Ok(found)
    }

    fn read_table() -> std::io::Result<Vec<u8>> {
        let mut size = 4 << 20;
        loop {
            let mut table = vec![0_u8; size];
            let mut needed = 0_u32;
            // SAFETY: `table` holds `size` bytes, which the call fills.
            let status = unsafe {
                NtQuerySystemInformation(
                    EXTENDED_HANDLE_INFORMATION,
                    table.as_mut_ptr().cast(),
                    size as u32,
                    &mut needed,
                )
            };
            if status >= 0 {
                return Ok(table);
            }
            if status != STATUS_INFO_LENGTH_MISMATCH || size >= TABLE_LIMIT {
                return Err(std::io::Error::other(format!(
                    "the handle table could not be read (status {status:#x})"
                )));
            }
            // Handles open while the table is read, so leave room to grow.
            size = (needed as usize + (1 << 20)).max(size * 2).min(TABLE_LIMIT);
        }
    }

    /// The path a handle of `owner` is open on, or `None` when it is not a
    /// file or folder on a disk.
    fn path_of(owner: HANDLE, handle: usize) -> Option<String> {
        let mut copy: HANDLE = INVALID_HANDLE_VALUE;
        // SAFETY: a duplicate into this process, closed below.
        let duplicated = unsafe {
            DuplicateHandle(
                owner,
                handle as HANDLE,
                GetCurrentProcess(),
                &mut copy,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if duplicated == 0 {
            return None;
        }
        let path = (|| {
            // SAFETY: `copy` is a valid handle of this process.
            if unsafe { GetFileType(copy) } != FILE_TYPE_DISK {
                return None;
            }
            let mut buffer = [0_u16; 1024];
            // SAFETY: the buffer holds the 1024 units the call is told of.
            let length = unsafe {
                GetFinalPathNameByHandleW(
                    copy,
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    FILE_NAME_NORMALIZED,
                )
            } as usize;
            (length > 0 && length < buffer.len())
                .then(|| String::from_utf16_lossy(&buffer[..length]))
        })();
        // SAFETY: the duplicate is closed once.
        unsafe { CloseHandle(copy) };
        path
    }

    fn process_name(pid: u32) -> String {
        // SAFETY: a plain process open; a null result means no access.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return "?".to_owned();
        }
        let mut buffer = [0_u16; 1024];
        let mut length = buffer.len() as u32;
        // SAFETY: the buffer holds the `length` units the call is told of.
        let named =
            unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
        // SAFETY: opened above and closed once.
        unsafe { CloseHandle(process) };
        if named == 0 {
            return "?".to_owned();
        }
        let full = String::from_utf16_lossy(&buffer[..length as usize]);
        full.rsplit('\\').next().unwrap_or(&full).to_owned()
    }

    /// A path in one spelling for comparing: no `\\?\` prefix, lower case, and
    /// a trailing separator, so a sibling whose name starts the same is not
    /// taken for something inside the folder.
    fn normalized(path: &str) -> String {
        let path = path.strip_prefix(r"\\?\").unwrap_or(path).to_lowercase();
        if path.ends_with('\\') {
            path
        } else {
            format!("{path}\\")
        }
    }
}
