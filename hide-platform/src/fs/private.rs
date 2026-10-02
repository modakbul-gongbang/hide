//! Files and folders only the current account can use, and the checks that
//! tell whether a folder can be trusted with code the account runs.
//!
//! "Private" is mode 0600 (0700 for a folder) on Unix. On Windows it is an
//! access list that names only the current account and does not inherit, set
//! when the file is created so there is no moment it is wider.

use std::fs;
use std::io;
use std::path::Path;

use super::Handle;

/// Makes the folder `path`, private. The parent must exist and `path` must
/// not (`AlreadyExists` otherwise), so the caller learns whether the folder
/// it is about to trust is one it made.
pub fn create_dir(path: &Path) -> io::Result<()> {
    sys::create_dir(path)
}

/// Makes the folder `path` and any missing folder above it, each private, the
/// way `fs::create_dir_all` makes them open. A folder that is already there
/// is left as it is, so the caller checks one it did not make before
/// trusting it.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_dir_all(parent)?;
    }
    match sys::create_dir(path) {
        // Another process made it between the look and the make.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        made => made,
    }
}

/// Makes the file `path`, private and open for writing. It must not exist.
pub fn create_new_file(path: &Path) -> io::Result<fs::File> {
    sys::open_file(path, true)
}

/// Opens the file `path` for writing without truncating it, making it
/// private when it does not exist. An existing file keeps what it has.
pub fn open_or_create_file(path: &Path) -> io::Result<fs::File> {
    sys::open_file(path, false)
}

/// Opens the account's own file `path` for reading, and for writing too when
/// `create` asks for it to be made, private, if it is not there. The name
/// itself is opened, never what a symbolic link at it leads to, and only a
/// regular file the current account owns and that has no other name is
/// accepted (`PermissionDenied` otherwise): the file may sit in a folder
/// other accounts write, where a planted link would have its target written
/// or read, or a planted pipe would block.
pub fn open_own_file(path: &Path, create: bool) -> io::Result<fs::File> {
    let file = sys::open_own(path, create)?;
    // A second name would be another account's hard link to one of this
    // account's files, planted where the caller looks.
    if !file.metadata()?.is_file()
        || !handle_owned_by_current_user(&file)?
        || super::identity::link_count(&file)? != 1
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the file is not the account's own regular file with one name",
        ));
    }
    Ok(file)
}

/// Makes an existing file or folder private: 0700 for a folder or a file its
/// owner can run, 0600 for any other file, on Unix. A folder's children made
/// after this are private too on Windows, which inherits the list; Unix has no
/// inheritance, and a new child takes the process's umask.
pub fn restrict_to_owner(path: &Path) -> io::Result<()> {
    sys::restrict_to_owner(path)
}

/// Whether nobody but the current account (and, on Windows, the system and
/// the administrators, who can take any file anyway) has any access to the
/// file or folder at `path`.
pub fn is_private(path: &Path) -> io::Result<bool> {
    sys::access_of_others(path, false).map(|others| !others)
}

/// Whether anybody but the current account (and the system on Windows) can
/// change the file or folder at `path`: write to it, delete it, or change who
/// may. A folder that answers yes cannot hold code the account runs, because
/// another account could replace that code.
pub fn others_can_modify(path: &Path) -> io::Result<bool> {
    sys::access_of_others(path, true)
}

/// Whether the file or folder at `path` belongs to the current account.
pub fn owned_by_current_user(path: &Path) -> io::Result<bool> {
    sys::owned_by_current_user(path)
}

/// Whether the open file or folder belongs to the current account.
pub fn handle_owned_by_current_user(handle: &impl Handle) -> io::Result<bool> {
    sys::handle_owned_by_current_user(handle)
}

#[cfg(unix)]
mod sys {
    use std::fs;
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::Path;

    use super::Handle;

    pub(super) fn create_dir(path: &Path) -> io::Result<()> {
        fs::DirBuilder::new().mode(0o700).create(path)
    }

    pub(super) fn open_file(path: &Path, new: bool) -> io::Result<fs::File> {
        let mut options = fs::OpenOptions::new();
        options.write(true).mode(0o600);
        if new {
            options.create_new(true);
        } else {
            options.create(true).truncate(false);
        }
        options.open(path)
    }

    pub(super) fn open_own(path: &Path, create: bool) -> io::Result<fs::File> {
        let mut options = fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        if create {
            options.write(true).create(true).truncate(false).mode(0o600);
        }
        options.open(path)
    }

    pub(super) fn restrict_to_owner(path: &Path) -> io::Result<()> {
        let metadata = fs::metadata(path)?;
        // A file that ran for its owner still does.
        let mode = if metadata.is_dir() || metadata.mode() & 0o100 != 0 {
            0o700
        } else {
            0o600
        };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }

    /// `changes` asks about write access, otherwise about any access.
    pub(super) fn access_of_others(path: &Path, changes: bool) -> io::Result<bool> {
        let watched = if changes { 0o022 } else { 0o077 };
        Ok(fs::metadata(path)?.mode() & watched != 0)
    }

    fn current_user() -> u32 {
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { libc::geteuid() }
    }

    pub(super) fn owned_by_current_user(path: &Path) -> io::Result<bool> {
        Ok(fs::metadata(path)?.uid() == current_user())
    }

    pub(super) fn handle_owned_by_current_user(handle: &impl Handle) -> io::Result<bool> {
        let metadata = crate::fs::duplicate(handle)?.metadata()?;
        Ok(metadata.uid() == current_user())
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    use std::fs;
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::Path;
    use std::ptr::{null, null_mut};

    use widestring::U16CString;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_SUCCESS, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
        LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetNamedSecurityInfoW, GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
        SetNamedSecurityInfoW,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation,
        GetSecurityDescriptorDacl, GetTokenInformation, INHERIT_ONLY_ACE,
        OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        PSID, SECURITY_ATTRIBUTES, SECURITY_MAX_SID_SIZE, TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER,
        TokenOwner, TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_ALWAYS, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    use super::Handle;
    use crate::fs::wide;

    /// Access that changes a file: write, append, delete, delete a child,
    /// change the access list or the owner, and the generic forms of those.
    const CHANGE: u32 = 0x0000_0002
        | 0x0000_0004
        | 0x0000_0010
        | 0x0000_0040
        | 0x0000_0100
        | 0x0001_0000
        | 0x0004_0000
        | 0x0008_0000
        | 0x4000_0000
        | 0x1000_0000;

    /// The ACE type of an entry that allows access.
    const ACCESS_ALLOWED: u8 = 0;
    /// The ACE types of a denial: plain, object, callback, callback object.
    const DENIALS: [u8; 4] = [1, 6, 0x0A, 0x0C];

    /// A kernel handle closed on drop.
    struct Closing(HANDLE);

    impl Drop for Closing {
        fn drop(&mut self) {
            // SAFETY: the handle is open and owned by this value.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// A buffer the system allocated with `LocalAlloc`, freed on drop.
    struct Local(*mut c_void);

    impl Drop for Local {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the pointer came from an API that allocates with
                // LocalAlloc and asks the caller to free it, and is freed once.
                unsafe { LocalFree(self.0) };
            }
        }
    }

    /// A SID owned as bytes, so it can be compared and printed.
    struct Sid(Vec<u8>);

    impl Sid {
        fn as_psid(&self) -> PSID {
            self.0.as_ptr().cast_mut().cast()
        }

        fn well_known(kind: i32) -> io::Result<Self> {
            let mut bytes = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
            let mut size = bytes.len() as u32;
            // SAFETY: `bytes` holds `size` writable bytes.
            if unsafe { CreateWellKnownSid(kind, null_mut(), bytes.as_mut_ptr().cast(), &mut size) }
                == 0
            {
                return Err(io::Error::last_os_error());
            }
            bytes.truncate(size as usize);
            Ok(Self(bytes))
        }

        fn same_as(&self, other: PSID) -> bool {
            // SAFETY: both pointers are valid SIDs for the duration of the call.
            unsafe { EqualSid(self.as_psid(), other) != 0 }
        }

        /// `S-1-5-21-...`, the form an access-list string names an account by.
        fn text(&self) -> io::Result<String> {
            let mut text = null_mut();
            // SAFETY: the SID is valid, and `text` receives a LocalAlloc string.
            if unsafe { ConvertSidToStringSidW(self.as_psid(), &mut text) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let text = Local(text.cast());
            // SAFETY: the call returned a NUL-terminated wide string.
            let wide = unsafe { U16CString::from_ptr_str(text.0.cast::<u16>()) };
            Ok(wide.to_string_lossy())
        }
    }

    /// The account this process runs as, and the owner a file it creates gets
    /// (the Administrators group for an elevated process).
    struct Account {
        user: Sid,
        owner: Sid,
    }

    fn token_sid(token: HANDLE, class: i32) -> io::Result<Sid> {
        let mut size = 0u32;
        // SAFETY: a size query with no buffer; the call fails and sets `size`.
        unsafe { GetTokenInformation(token, class, null_mut(), 0, &mut size) };
        // A `u64` buffer keeps the structure the call writes aligned.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8).max(1)];
        // SAFETY: `buffer` holds at least `size` writable bytes.
        if unsafe { GetTokenInformation(token, class, buffer.as_mut_ptr().cast(), size, &mut size) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        // TOKEN_USER and TOKEN_OWNER both start with the pointer to the SID.
        // SAFETY: the call filled the structure for this class, and the SID it
        // points at lies inside `buffer`.
        let sid: PSID = if class == TokenUser {
            unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid }
        } else {
            unsafe { (*buffer.as_ptr().cast::<TOKEN_OWNER>()).Owner }
        };
        // SAFETY: `sid` is a valid SID inside `buffer`; its length is read from
        // the SID's own header and copied out.
        let length = unsafe { windows_sys::Win32::Security::GetLengthSid(sid) } as usize;
        // SAFETY: as above, `length` bytes are readable at `sid`.
        Ok(Sid(unsafe {
            std::slice::from_raw_parts(sid.cast::<u8>(), length)
        }
        .to_vec()))
    }

    fn account() -> io::Result<Account> {
        let mut token = null_mut();
        // SAFETY: the current-process pseudo handle is always valid.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Closing(token);
        Ok(Account {
            user: token_sid(token.0, TokenUser)?,
            owner: token_sid(token.0, TokenOwner)?,
        })
    }

    /// A security descriptor made from an access-list string, and the
    /// attributes that hand it to a create call.
    struct Descriptor {
        _local: Local,
        attributes: SECURITY_ATTRIBUTES,
    }

    /// An access list that gives the current account everything and nobody
    /// else anything, with inheritance cut off. A folder's children inherit
    /// it.
    fn private_descriptor(folder: bool) -> io::Result<Descriptor> {
        let user = account()?.user.text()?;
        let inherit = if folder { "OICI" } else { "" };
        let text = U16CString::from_str(format!("D:P(A;{inherit};FA;;;{user})"))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: `text` is NUL-terminated and `descriptor` receives a
        // LocalAlloc buffer.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Descriptor {
            _local: Local(descriptor),
            attributes: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
        })
    }

    pub(super) fn create_dir(path: &Path) -> io::Result<()> {
        let descriptor = private_descriptor(true)?;
        let name = wide(path)?;
        // SAFETY: `name` is NUL-terminated and the attributes outlive the call.
        if unsafe { CreateDirectoryW(name.as_ptr(), &descriptor.attributes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn open_file(path: &Path, new: bool) -> io::Result<fs::File> {
        let descriptor = private_descriptor(false)?;
        create_file(
            path,
            GENERIC_WRITE,
            Some(&descriptor),
            if new { CREATE_NEW } else { OPEN_ALWAYS },
            FILE_ATTRIBUTE_NORMAL,
        )
    }

    pub(super) fn open_own(path: &Path, create: bool) -> io::Result<fs::File> {
        // A link at the name is opened as itself, and refused by the caller
        // as a file that is not regular.
        if create {
            let descriptor = private_descriptor(false)?;
            create_file(
                path,
                GENERIC_READ | GENERIC_WRITE,
                Some(&descriptor),
                OPEN_ALWAYS,
                FILE_FLAG_OPEN_REPARSE_POINT,
            )
        } else {
            create_file(
                path,
                GENERIC_READ,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
            )
        }
    }

    /// `CreateFileW`, sharing everything, with the private descriptor for a
    /// file it makes.
    fn create_file(
        path: &Path,
        access: u32,
        descriptor: Option<&Descriptor>,
        disposition: u32,
        flags: u32,
    ) -> io::Result<fs::File> {
        let name = wide(path)?;
        let attributes = descriptor.map_or(null(), |descriptor| &descriptor.attributes);
        // SAFETY: `name` is NUL-terminated and the attributes, when given,
        // outlive the call.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                attributes,
                disposition,
                flags,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call returned a new handle that nothing else owns.
        Ok(unsafe { fs::File::from_raw_handle(handle) })
    }

    pub(super) fn restrict_to_owner(path: &Path) -> io::Result<()> {
        let folder = fs::metadata(path)?.is_dir();
        let descriptor = private_descriptor(folder)?;
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl: *mut ACL = null_mut();
        // SAFETY: the descriptor is valid and the out parameters are writable.
        if unsafe {
            GetSecurityDescriptorDacl(
                descriptor.attributes.lpSecurityDescriptor,
                &mut present,
                &mut acl,
                &mut defaulted,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if present == 0 {
            // A descriptor without an access list would be set as one that
            // gives everybody everything.
            return Err(io::Error::other("the private access list has no entries"));
        }
        let name = wide(path)?;
        // SAFETY: `name` is NUL-terminated and `acl` lives in the descriptor
        // that outlives the call.
        let status = unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                acl,
                null(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        Ok(())
    }

    /// The security a path or an open file carries, and who owns it.
    struct Security {
        _descriptor: Local,
        owner: PSID,
        acl: *mut ACL,
    }

    fn security_of_path(path: &Path) -> io::Result<Security> {
        let name = wide(path)?;
        let mut owner = null_mut();
        let mut acl = null_mut();
        let mut descriptor = null_mut();
        // SAFETY: `name` is NUL-terminated and the out parameters are writable.
        let status = unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        Ok(Security {
            _descriptor: Local(descriptor),
            owner,
            acl,
        })
    }

    fn security_of_handle(handle: &impl Handle) -> io::Result<Security> {
        let raw = handle.as_handle().as_raw_handle();
        let mut owner = null_mut();
        let mut acl = null_mut();
        let mut descriptor = null_mut();
        // SAFETY: `raw` is an open handle borrowed for the call and the out
        // parameters are writable.
        let status = unsafe {
            GetSecurityInfo(
                raw,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        Ok(Security {
            _descriptor: Local(descriptor),
            owner,
            acl,
        })
    }

    fn owned(security: &Security) -> io::Result<bool> {
        let account = account()?;
        Ok(account.user.same_as(security.owner) || account.owner.same_as(security.owner))
    }

    pub(super) fn owned_by_current_user(path: &Path) -> io::Result<bool> {
        owned(&security_of_path(path)?)
    }

    pub(super) fn handle_owned_by_current_user(handle: &impl Handle) -> io::Result<bool> {
        owned(&security_of_handle(handle)?)
    }

    /// Whether an access list the current account does not own grants `others`
    /// any access (`changes` false) or any access that changes the file.
    pub(super) fn access_of_others(path: &Path, changes: bool) -> io::Result<bool> {
        let security = security_of_path(path)?;
        let account = account()?;
        let trusted = [
            account.user,
            account.owner,
            Sid::well_known(WinLocalSystemSid)?,
            Sid::well_known(WinBuiltinAdministratorsSid)?,
        ];
        if security.acl.is_null() {
            // No access list at all means everybody has every access.
            return Ok(true);
        }
        let mut size = ACL_SIZE_INFORMATION {
            AceCount: 0,
            AclBytesInUse: 0,
            AclBytesFree: 0,
        };
        // SAFETY: the list is valid and `size` is the structure the class names.
        if unsafe {
            GetAclInformation(
                security.acl,
                (&mut size as *mut ACL_SIZE_INFORMATION).cast(),
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        for index in 0..size.AceCount {
            let mut entry: *mut c_void = null_mut();
            // SAFETY: `index` is below the entry count of a valid list.
            if unsafe { GetAce(security.acl, index, &mut entry) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: every entry starts with its header.
            let header = unsafe { &*entry.cast::<ACE_HEADER>() };
            if u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 {
                // It only applies to the children of a folder.
                continue;
            }
            // Denials take access away and need no check; an entry that is
            // neither a plain allowance nor a denial (an object, callback or
            // compound allowance) is counted as access, since not knowing is
            // not the same as nobody having it.
            if header.AceType != ACCESS_ALLOWED {
                if DENIALS.contains(&header.AceType) {
                    continue;
                }
                return Ok(true);
            }
            // SAFETY: an allow entry has this layout; the SID starts at
            // `SidStart` and runs to the end of the entry.
            let allowed = unsafe { &*entry.cast::<ACCESS_ALLOWED_ACE>() };
            let sid: PSID = (&allowed.SidStart as *const u32).cast_mut().cast();
            if trusted.iter().any(|known| known.same_as(sid)) {
                continue;
            }
            if (!changes && allowed.Mask != 0) || allowed.Mask & CHANGE != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
