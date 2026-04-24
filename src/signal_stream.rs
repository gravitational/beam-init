use std::ffi::c_int;
use std::fs::File;
use std::io::{self, Read};
use std::mem::MaybeUninit;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::{mem, ptr};

use libc::{
    SFD_CLOEXEC, SFD_NONBLOCK, SIG_BLOCK, SIG_SETMASK, pthread_sigmask, sigaddset, sigemptyset,
    signalfd, signalfd_siginfo, sigset_t,
};
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;

use crate::Event;
use crate::system::cerr;

pub fn init(signals: &[c_int], tx_event: mpsc::Sender<Event>) -> io::Result<OldSigmask> {
    // SAFETY: This is a valid way to initialize a sigset_t.
    let signal_set = unsafe {
        let mut signal_set: MaybeUninit<sigset_t> = MaybeUninit::uninit();
        cerr(sigemptyset(signal_set.as_mut_ptr()))?;
        for &signum in signals {
            cerr(sigaddset(signal_set.as_mut_ptr(), signum))?;
        }
        signal_set.assume_init()
    };

    // SAFETY: `signalfd`` is passed a valid signal set pointer and returns an owned fd.
    let rx = unsafe {
        OwnedFd::from_raw_fd(cerr(signalfd(-1, &signal_set, SFD_CLOEXEC | SFD_NONBLOCK))?)
    };
    let mut rx = AsyncFd::new(File::from(rx))?;

    // SAFETY: `pthread_sigmask` is passed a valid pointer to a signal set and
    // a mutable pointer to an uninitialized signal set it will initialize.
    let old_sigmask = unsafe {
        let mut old_sigmask: MaybeUninit<sigset_t> = MaybeUninit::uninit();
        cerr(pthread_sigmask(
            SIG_BLOCK,
            &signal_set,
            old_sigmask.as_mut_ptr(),
        ))?;
        old_sigmask.assume_init()
    };

    tokio::spawn(async move {
        loop {
            let mut siginfo = [0; size_of::<signalfd_siginfo>()];
            rx.async_io_mut(Interest::READABLE, |inner| inner.read_exact(&mut siginfo))
                .await
                .unwrap();
            // SAFETY: `signalfd_siginfo` does not contain any padding or
            // pointers, nor does `[u8; _]`. And `signalfd_siginfo` doesn't
            // have any private fields with invariants.
            let siginfo = unsafe { mem::transmute::<[u8; _], signalfd_siginfo>(siginfo) };
            if tx_event.send(Event::Signal(siginfo)).await.is_err() {
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
        // SAFETY: pthread_sigmask is an async signal safe function
        unsafe {
            cmd.pre_exec(move || {
                // SAFETY: A valid sigset_t pointer is passed to pthread_sigmask.
                cerr(pthread_sigmask(SIG_SETMASK, &old_sigmask, ptr::null_mut()))?;
                Ok(())
            })
        }
    }
}
