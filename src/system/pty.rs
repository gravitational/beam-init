use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::system::cerr;

#[derive(Debug)]
pub struct Pty {
    master: OwnedFd,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct PtyClient<'a> {
    parent: &'a mut Pty,
}

impl Pty {
    pub fn new() -> io::Result<Pty> {
        let flags = libc::O_RDWR | libc::O_NOCTTY;

        // SAFETY:
        // - libc::posix_openpt is safe to call
        // - if it doesn't return -1, it returns a valid file descriptor for from_raw_fd
        let master = unsafe { OwnedFd::from_raw_fd(cerr(libc::posix_openpt(flags))?) };

        let mut buffer = [0u8; libc::PATH_MAX as usize];
        let pts_name = {
            // SAFETY: ptsname_r is passed pointers to correct memory; no other assumptions are made
            let err = unsafe {
                libc::ptsname_r(master.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len())
            };
            // "On success, ptsname_r() returns 0. On failure, an error number is returned to indicate the error."
            // i.e. we cannot wrap the call to libc::ptsname_r in cerr() since that only considers -1 an error and
            // expects the actual error value to be in errno (which the manpage doesn't guarantee for ptsname_r)
            if err != 0 {
                return Err(io::Error::from_raw_os_error(err));
            }

            let c_str = CStr::from_bytes_until_nul(&buffer)
                .expect("CStr conversion should not fail")
                .to_bytes();

            Path::new(OsStr::from_bytes(c_str))
        };

        Ok(Pty {
            master,
            path: pts_name.to_owned(),
        })
    }

    pub fn client(&mut self) -> PtyClient<'_> {
        PtyClient { parent: self }
    }
}

impl<'a> PtyClient<'a> {
    /// Associate the client side of the PTY to the current process
    pub fn make_tty(self) -> io::Result<OwnedFd> {
        // SAFETY: these functions are safe to call (and are being fed the correct file descriptor)
        unsafe {
            cerr(libc::grantpt(self.parent.master.as_raw_fd()))?;
            cerr(libc::unlockpt(self.parent.master.as_raw_fd()))?;
        }

        let path = CString::new(self.parent.path.as_os_str().as_bytes())
            .expect("PTY path to not have null bytes");

        // SAFETY:
        // - libc::open is passed a correct null-terminated C string
        // - only if the fd is opened correctly is it passed to from_raw_fd
        let client = unsafe {
            // NOTE: Opening terminal device makes that the controlling terminal for this session;
            // so by not passing O_NOCTTY we can avoid the TIOCSCTTY ioctl
            let fd = cerr(libc::open(path.as_ptr(), libc::O_RDWR))?;
            OwnedFd::from_raw_fd(fd)
        };

        Ok(client)
    }
}
