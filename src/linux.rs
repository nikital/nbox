use std::{
    ffi::{CStr, OsStr, OsString, c_char, c_int, c_uint},
    io,
    mem::MaybeUninit,
    os::unix::ffi::OsStrExt,
    ptr,
};

pub(crate) unsafe fn open_tree(dirfd: c_int, path: &CStr, flags: c_uint) -> io::Result<c_int> {
    unsafe {
        let ret = libc::syscall(libc::SYS_open_tree, dirfd, path.as_ptr(), flags);
        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(ret as _)
        }
    }
}

pub(crate) unsafe fn mount_setattr(
    dirfd: c_int,
    path: &CStr,
    flags: c_uint,
    attr: &mut libc::mount_attr,
) -> io::Result<()> {
    unsafe {
        let ret = libc::syscall(
            libc::SYS_mount_setattr,
            dirfd,
            path.as_ptr(),
            flags,
            attr as *mut libc::mount_attr,
            std::mem::size_of_val(attr),
        );
        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

pub(crate) unsafe fn move_mount(
    from_dirfd: c_int,
    from_path: &CStr,
    to_dirfd: c_int,
    to_path: &CStr,
    flags: c_uint,
) -> io::Result<()> {
    unsafe {
        let ret = libc::syscall(
            libc::SYS_move_mount,
            from_dirfd,
            from_path.as_ptr(),
            to_dirfd,
            to_path.as_ptr(),
            flags,
        );
        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

pub(crate) struct Passwd {
    pub(crate) pw_name: OsString,
    pub(crate) pw_uid: libc::uid_t,
    #[expect(dead_code)]
    pub(crate) pw_gid: libc::gid_t,
    pub(crate) pw_dir: OsString,
    #[expect(dead_code)]
    pub(crate) pw_shell: OsString,
}

pub(crate) fn getpwuid_r(uid: libc::uid_t) -> eyre::Result<Option<Passwd>> {
    unsafe {
        let mut buf: Vec<c_char> = Vec::with_capacity(0x10_000);
        let mut pwd = MaybeUninit::<libc::passwd>::uninit();
        let mut result: *mut libc::passwd = ptr::null_mut();
        let ret = libc::getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            buf.as_mut_ptr(),
            buf.capacity(),
            &mut result,
        );
        if ret != 0 {
            return Err(io::Error::from_raw_os_error(ret).into());
        }
        if result.is_null() {
            return Ok(None);
        }
        let result = &*result;
        return Ok(Some(Passwd {
            pw_name: OsStr::from_bytes(CStr::from_ptr(result.pw_name).to_bytes()).to_owned(),
            pw_uid: result.pw_uid,
            pw_gid: result.pw_gid,
            pw_dir: OsStr::from_bytes(CStr::from_ptr(result.pw_dir).to_bytes()).to_owned(),
            pw_shell: OsStr::from_bytes(CStr::from_ptr(result.pw_shell).to_bytes()).to_owned(),
        }));
    }
}
