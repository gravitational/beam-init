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
use tokio::sync::mpsc;

use crate::Event;
use crate::system::cerr;

pub unsafe fn init(signals: &[c_int], tx_event: mpsc::Sender<Event>) -> io::Result<OldSigmask> {
    let (mut rx, old_sigmask) = unsafe {
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
        let rx = AsyncFd::new(File::from(rx))?;

        let mut old_sigmask: MaybeUninit<sigset_t> = MaybeUninit::uninit();
        cerr(sigprocmask(
            SIG_BLOCK,
            signal_set.as_ptr(),
            old_sigmask.as_mut_ptr(),
        ))?;

        (rx, old_sigmask.assume_init())
    };

    tokio::spawn(async move {
        loop {
            let mut siginfo = [0; size_of::<signalfd_siginfo>()];
            rx.async_io_mut(Interest::READABLE, |inner| inner.read_exact(&mut siginfo))
                .await
                .unwrap();
            if tx_event
                .send(Event::Signal(unsafe {
                    mem::transmute::<[u8; _], signalfd_siginfo>(siginfo)
                }))
                .await
                .is_err()
            {
                return; // Main event loop has finished
            }
        }
    });

    Ok(OldSigmask(old_sigmask))
}

pub struct OldSigmask(sigset_t);

impl OldSigmask {
    pub fn with_restored_sigmask<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        let old_sigmask = self.0;
        unsafe {
            cmd.pre_exec(move || {
                cerr(sigprocmask(SIG_SETMASK, &old_sigmask, ptr::null_mut()))?;
                Ok(())
            })
        }
    }
}
