use std::fs::File;
use std::io::{self, Read};
use std::mem;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::prelude::{BorrowedFd, RawFd};

use libc::{SFD_CLOEXEC, SFD_NONBLOCK, signalfd, signalfd_siginfo};

use crate::system::cerr;
use crate::system::signal_set::SignalSet;

pub struct SignalFd(File);

impl SignalFd {
    pub fn new(signal_set: &SignalSet) -> io::Result<Self> {
        // -1 indicates creating a new signalfd receiving the given signals.
        // SAFETY: `signalfd` is passed a valid signal set pointer and returns an owned fd.
        let rx = unsafe {
            OwnedFd::from_raw_fd(cerr(signalfd(
                -1,
                signal_set.as_ref(),
                SFD_CLOEXEC | SFD_NONBLOCK,
            ))?)
        };
        Ok(SignalFd(File::from(rx)))
    }

    pub fn read(&mut self) -> io::Result<signalfd_siginfo> {
        let mut siginfo = [0; size_of::<signalfd_siginfo>()];
        self.0.read_exact(&mut siginfo)?;

        // SAFETY: `signalfd_siginfo` does not contain any padding or
        // pointers, nor does `[u8; _]`. And `signalfd_siginfo` doesn't
        // have any private fields with invariants.
        Ok(unsafe { mem::transmute::<[u8; _], signalfd_siginfo>(siginfo) })
    }
}

impl AsFd for SignalFd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl AsRawFd for SignalFd {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}
