//! The owner rule of the read scope base.
//!
//! The read scope base of a project (`fallow_config::project_scope_base`) can
//! be a directory above the analysis root, such as the Git work tree of a
//! package root. A directory above the root widens the scope only when other
//! users cannot write to it and one of these users owns it:
//!
//! - the current user (the effective uid), or
//! - the user that owns the analysis root.
//!
//! So a CI job that runs as root on a checkout of another user still reads
//! the shared configs of that checkout. A `/tmp/.git` entry, or a workspace
//! manifest in a shared directory, does not widen the scope of a project
//! below it, because a different user owns it or all users can write to it.
//!
//! On Windows the rule compares owner SIDs: a directory above the root widens
//! the scope only when its owner is the user of the current process or the
//! owner of the analysis root. So a `.git` entry in `C:\Users\Public` does not
//! widen the scope of a project below it. The Windows rule has no check of
//! the write access of other users, because the default ACLs of a user folder
//! often grant that access and the check would drop the monorepo scope for
//! most Windows users.

use std::path::Path;

/// The mode bit that lets all users write to a directory.
#[cfg(unix)]
const WRITABLE_BY_ALL: u32 = 0o002;

/// True when the directory `dir` above the analysis root `root` may widen
/// the read scope: other users cannot write to `dir`, and the current user
/// or the owner of `root` owns `dir`. A metadata error counts as false. On
/// Windows the rule does not apply, and the result is true.
#[cfg(unix)]
#[must_use]
pub fn may_widen_scope(dir: &Path, root: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Ok(dir_meta), Ok(root_meta)) = (std::fs::metadata(dir), std::fs::metadata(root)) else {
        return false;
    };
    owner_rule_allows(
        dir_meta.uid(),
        dir_meta.mode(),
        root_meta.uid(),
        rustix::process::geteuid().as_raw(),
    )
}

/// True when the directory `dir` above the analysis root `root` may widen
/// the read scope: the user of the current process or the owner of `root`
/// owns `dir`. An error of an owner lookup counts as false.
#[cfg(windows)]
#[must_use]
pub fn may_widen_scope(dir: &Path, root: &Path) -> bool {
    let (Some(dir_owner), Some(root_owner), Some(user)) = (
        windows_owner::owner_sid(dir),
        windows_owner::owner_sid(root),
        windows_owner::process_user_sid(),
    ) else {
        return false;
    };
    sid_owner_rule_allows(&dir_owner, &root_owner, &user)
}

/// True when the directory `dir` above the analysis root `root` may widen
/// the read scope. On a target without an owner model the rule does not
/// apply, and the result is true.
#[cfg(not(any(unix, windows)))]
#[must_use]
pub fn may_widen_scope(_dir: &Path, _root: &Path) -> bool {
    true
}

/// The Windows owner rule over the binary owner SID `dir_owner` of a
/// directory, the owner SID `root_owner` of the analysis root, and the SID
/// `user` of the process user. Two valid SIDs are equal exactly when their
/// bytes are equal, so a byte compare gives the result of `EqualSid`.
#[cfg(any(windows, test))]
fn sid_owner_rule_allows(dir_owner: &[u8], root_owner: &[u8], user: &[u8]) -> bool {
    dir_owner == user || dir_owner == root_owner
}

/// The Win32 owner lookups of the Windows owner rule. Each lookup copies the
/// binary SID out of the memory that Windows owns, and returns `None` on an
/// error.
#[cfg(windows)]
#[expect(unsafe_code, reason = "the owner SID lookups require Win32 FFI calls")]
mod windows_owner {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        GetLengthSid, GetTokenInformation, IsValidSid, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// The binary owner SID of the file or directory `path`.
    pub(super) fn owner_sid(path: &Path) -> Option<Vec<u8>> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return None;
        }
        wide.push(0);
        let mut owner: PSID = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `wide` is a NUL-terminated UTF-16 string that lives for the
        // call. The out pointers point to live locals, and the call accepts a
        // null pointer for the group, the DACL and the SACL.
        let status = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &raw mut owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let sid = copy_sid(owner);
        // SAFETY: `descriptor` is the buffer that GetNamedSecurityInfoW
        // allocated for this call. `owner` points into it, and `copy_sid`
        // does not keep that pointer, so nothing reads it after the free.
        unsafe { LocalFree(descriptor) };
        sid
    }

    /// The binary SID of the user of the current process token.
    pub(super) fn process_user_sid() -> Option<Vec<u8>> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a pseudo handle that needs no
        // close.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: `token` is a live local that receives the token handle.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
            return None;
        }
        let sid = token_user_sid(token);
        // SAFETY: `token` is the handle that OpenProcessToken opened above,
        // and this is its only close.
        unsafe { CloseHandle(token) };
        sid
    }

    /// The binary SID in the `TokenUser` information of the token `token`.
    fn token_user_sid(token: HANDLE) -> Option<Vec<u8>> {
        let mut len: u32 = 0;
        // SAFETY: A size query with a null buffer and a zero length writes
        // only the required length into `len`. The call fails by design, so
        // the code reads only `len`.
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &raw mut len) };
        let byte_len = usize::try_from(len).ok()?;
        if byte_len < std::mem::size_of::<TOKEN_USER>() {
            return None;
        }
        // A `u64` buffer gives the pointer alignment that TOKEN_USER needs.
        let mut buffer = vec![0_u64; byte_len.div_ceil(8)];
        // SAFETY: `buffer` holds at least `len` writable bytes and lives for
        // the call.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                len,
                &raw mut len,
            )
        };
        if ok == 0 {
            return None;
        }
        // SAFETY: The call above filled `buffer` with a TOKEN_USER value, and
        // the buffer is aligned for it and at least as large as it.
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        copy_sid(user.User.Sid)
    }

    /// A copy of the bytes of the SID at `sid`, or `None` when the SID is
    /// null or not valid.
    fn copy_sid(sid: PSID) -> Option<Vec<u8>> {
        if sid.is_null() {
            return None;
        }
        // SAFETY: `sid` is a non-null SID pointer from a Win32 call, and its
        // owner buffer is live for this function.
        if unsafe { IsValidSid(sid) } == 0 {
            return None;
        }
        // SAFETY: IsValidSid accepted `sid` above.
        let len = usize::try_from(unsafe { GetLengthSid(sid) }).ok()?;
        // SAFETY: A valid SID spans `len` readable bytes, as GetLengthSid
        // reports.
        let bytes = unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), len) };
        Some(bytes.to_vec())
    }
}

/// The owner rule over the owner `dir_uid` and the mode `dir_mode` of a
/// directory, the owner `root_uid` of the analysis root, and the effective
/// uid `euid`.
#[cfg(unix)]
const fn owner_rule_allows(dir_uid: u32, dir_mode: u32, root_uid: u32, euid: u32) -> bool {
    dir_mode & WRITABLE_BY_ALL == 0 && (dir_uid == euid || dir_uid == root_uid)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    const PRIVATE_DIR: u32 = 0o040_755;
    const SHARED_DIR: u32 = 0o041_777;

    #[test]
    fn a_directory_of_the_current_user_widens_the_scope() {
        assert!(owner_rule_allows(501, PRIVATE_DIR, 1001, 501));
    }

    #[test]
    fn a_directory_of_the_root_owner_widens_the_scope_for_another_user() {
        assert!(owner_rule_allows(1001, PRIVATE_DIR, 1001, 0));
    }

    #[test]
    fn a_directory_of_a_third_user_does_not_widen_the_scope() {
        assert!(!owner_rule_allows(2002, PRIVATE_DIR, 1001, 0));
    }

    #[test]
    fn a_directory_that_all_users_can_write_never_widens_the_scope() {
        assert!(!owner_rule_allows(501, SHARED_DIR, 501, 501));
        assert!(!owner_rule_allows(1001, SHARED_DIR, 1001, 0));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_directory_that_all_users_can_write_does_not_widen_the_scope() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("root");
        std::fs::create_dir(&root).expect("root dir");
        assert!(may_widen_scope(dir.path(), &root));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777))
            .expect("chmod");
        assert!(!may_widen_scope(dir.path(), &root));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("chmod");
    }

    /// `/usr` belongs to the system user on each Unix host.
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_directory_that_another_user_owns_does_not_widen_the_scope() {
        if rustix::process::geteuid().is_root() {
            return;
        }
        let root = tempfile::tempdir().expect("temp dir");
        assert!(!may_widen_scope(Path::new("/usr"), root.path()));
    }
}

#[cfg(test)]
mod sid_tests {
    use super::sid_owner_rule_allows;

    /// The binary SIDs `S-1-5-21-1-2-3-1001`, `-1002` and `-1003`.
    const fn user_sid(rid: u8) -> [u8; 28] {
        [
            1, 5, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, rid, 3, 0, 0,
        ]
    }

    #[test]
    fn a_directory_of_the_process_user_widens_the_scope() {
        assert!(sid_owner_rule_allows(
            &user_sid(1),
            &user_sid(2),
            &user_sid(1)
        ));
    }

    #[test]
    fn a_directory_of_the_root_owner_widens_the_scope_for_another_user() {
        assert!(sid_owner_rule_allows(
            &user_sid(2),
            &user_sid(2),
            &user_sid(1)
        ));
    }

    #[test]
    fn a_directory_of_a_third_user_does_not_widen_the_scope() {
        assert!(!sid_owner_rule_allows(
            &user_sid(3),
            &user_sid(2),
            &user_sid(1)
        ));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::may_widen_scope;

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_directory_of_the_current_user_widens_the_scope_of_a_root_below_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("root");
        std::fs::create_dir(&root).expect("root dir");
        assert!(may_widen_scope(dir.path(), &root));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_path_without_an_owner_does_not_widen_the_scope() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("missing");
        assert!(!may_widen_scope(&missing, dir.path()));
    }
}
