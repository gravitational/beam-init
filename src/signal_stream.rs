// FIXME replace this with a generic signal stream interface

use std::ffi::c_int;
use std::fs::File;
use std::io::{self, Read};
use std::mem::MaybeUninit;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::{mem, ptr};

use libc::{
    SFD_CLOEXEC, SFD_NONBLOCK, SIG_BLOCK, SIG_SETMASK, sigaddset, sigemptyset, signalfd,
    signalfd_siginfo, sigprocmask, sigset_t,
};
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;

use crate::system::cerr;

pub unsafe fn init(signals: &[c_int]) -> io::Result<SignalStream> {
    unsafe {
        let mut signal_set: MaybeUninit<sigset_t> = MaybeUninit::uninit();
        cerr(sigemptyset(signal_set.as_mut_ptr()))?;
        for &signum in signals {
            cerr(sigaddset(signal_set.as_mut_ptr(), signum))?;
        }

        let rx = OwnedFd::from_raw_fd(cerr(signalfd(
            -1,
            signal_set.as_ptr(),
            SFD_CLOEXEC | SFD_NONBLOCK,
        ))?);
        let mut old_sigmask: MaybeUninit<sigset_t> = MaybeUninit::uninit();
        cerr(sigprocmask(
            SIG_BLOCK,
            signal_set.as_ptr(),
            old_sigmask.as_mut_ptr(),
        ))?;

        Ok(SignalStream {
            rx: AsyncFd::new(File::from(rx))?,
            old_sigmask: old_sigmask.assume_init(),
        })
    }
}

pub struct SignalStream {
    rx: AsyncFd<File>,
    old_sigmask: sigset_t,
}

impl SignalStream {
    // Restarting this function after a cancel is idempotent.
    pub async fn recv(&mut self) -> io::Result<signalfd_siginfo> {
        let mut siginfo = [0; size_of::<signalfd_siginfo>()];
        self.rx
            .async_io_mut(Interest::READABLE, |inner| inner.read_exact(&mut siginfo))
            .await?;
        Ok(unsafe { mem::transmute::<[u8; _], signalfd_siginfo>(siginfo) })
    }

    pub fn with_restored_sigmask<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        let old_sigmask = self.old_sigmask;
        unsafe {
            cmd.pre_exec(move || {
                cerr(sigprocmask(SIG_SETMASK, &old_sigmask, ptr::null_mut()))?;
                Ok(())
            })
        }
    }
}
