use std::ffi::{CStr, OsStr};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::system::cerr;

#[derive(Debug)]
pub struct Pty {
    master: OwnedFd,
    pub path: PathBuf,
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

    pub fn grant(&self) -> io::Result<OwnedFd> {
        let pty_fd = self.master.as_raw_fd();

        // SAFETY: these functions are a safe to call (and are being fed the correct file descriptor)
        unsafe {
            cerr(libc::grantpt(pty_fd))?;
            cerr(libc::unlockpt(pty_fd))?;
        }

        let mut options = OpenOptions::new();
        options.write(true);
        options.read(true);
        options.custom_flags(libc::O_NOCTTY);
        let client = OwnedFd::from(File::options().read(true).write(true).open(&self.path)?);

        Ok(client)
    }
}

impl std::fmt::Display for Pty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path.display())
    }
}
