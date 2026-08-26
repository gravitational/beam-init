use std::ffi::{CStr, OsStr, c_int};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use libc::{TIOCGPTPEER, TIOCSPTLCK, uid_t};

use crate::system::cerr;

#[derive(Debug)]
pub struct Pty<T> {
    pub master: T,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct PtyClient<'a, T> {
    parent: &'a mut Pty<T>,
}

impl<T: AsFd> Pty<T> {
    pub fn new(map_fd: impl FnOnce(OwnedFd) -> T) -> io::Result<Self> {
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

            let c_str =
                CStr::from_bytes_until_nul(&buffer).expect("CStr conversion should not fail");

            Path::new(OsStr::from_bytes(c_str.to_bytes()))
        };

        Ok(Pty {
            master: map_fd(master),
            path: pts_name.to_owned(),
        })
    }

    pub fn client(&mut self) -> PtyClient<'_, T> {
        PtyClient { parent: self }
    }
}

impl<'a, T: AsFd> PtyClient<'a, T> {
    /// Associate the client side of the PTY to the current process
    ///
    /// The given uid will be the owner of the client side of the PTY.
    pub fn make_tty(self, uid: uid_t) -> io::Result<OwnedFd> {
        let master = self.parent.master.as_fd();

        // Equivalent to unlockpt, but async-signal-safe
        // SAFETY: this ioctl is safe to call (and is being fed the correct file descriptor)
        unsafe {
            cerr(libc::ioctl(master.as_raw_fd(), TIOCSPTLCK, &(0 as c_int)))?;
        }

        // Equivalent to opening the result of ptsname, except works even when
        // devpts is not mounted at the expected location.
        // SAFETY:
        // - this ioctl is safe to call (and is being fed the correct file descriptor)
        // - only if the fd is opened correctly is it passed to from_raw_fd
        let client = unsafe {
            // NOTE: Opening terminal device makes that the controlling terminal for this session;
            // so by not passing O_NOCTTY we can avoid the TIOCSCTTY ioctl
            let fd = cerr(libc::ioctl(master.as_raw_fd(), TIOCGPTPEER, libc::O_RDWR))?;
            OwnedFd::from_raw_fd(fd)
        };

        // Get the existing gid. This is presumed to be the tty group.
        let mut stat = MaybeUninit::<libc::stat>::uninit();
        // SAFETY: A valid fd and pointer are passed to fstat and fstat initializes stat.
        let stat = unsafe {
            cerr(libc::fstat(client.as_raw_fd(), stat.as_mut_ptr()))?;
            stat.assume_init()
        };

        // Set the owner of the client side of the pty. Theoretically grantpt
        // should work, but in our case this runs before changing user and in
        // addition, glibc and musl don't actually implement grantpt. Instead
        // they just assume that the pty is created by the user who will access
        // the pty client and merely checks if the fd is a valid pty in grantpt.
        // SAFETY: this function is safe to call (and is being fed the correct file descriptor)
        unsafe {
            cerr(libc::fchown(client.as_raw_fd(), uid, stat.st_gid))?;
        }

        Ok(client)
    }
}
