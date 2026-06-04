use std::ffi::c_int;
use std::fs::File;
use std::io::{self, Read};
use std::mem;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::Command;

use libc::{SFD_CLOEXEC, SFD_NONBLOCK, signalfd, signalfd_siginfo};
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;

use crate::Event;
use crate::system::cerr;
use crate::system::signal_set::SignalSet;

pub fn init(signals: &[c_int], tx_event: mpsc::Sender<Event>) -> io::Result<OldSigmask> {
    let mut signal_set = SignalSet::empty()?;
    for &signum in signals {
        signal_set.add(signum)?;
    }

    // -1 indicates creating a new signalfd receiving the given signals.
    // SAFETY: `signalfd` is passed a valid signal set pointer and returns an owned fd.\
    let rx = unsafe {
        OwnedFd::from_raw_fd(cerr(signalfd(
            -1,
            signal_set.as_ref(),
            SFD_CLOEXEC | SFD_NONBLOCK,
        ))?)
    };
    let mut rx = AsyncFd::new(File::from(rx))?;

    let old_sigmask = signal_set.block()?;

    tokio::spawn(async move {
        loop {
            let mut siginfo = [0; size_of::<signalfd_siginfo>()];
            rx.async_io_mut(Interest::READABLE, |inner| inner.read_exact(&mut siginfo))
                .await
                .expect("failed to read signal from signalfd");
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

#[derive(Copy, Clone)]
pub struct OldSigmask(SignalSet);

impl OldSigmask {
    pub fn with_restored_sigmask<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        let old_sigmask = self.0;
        // SAFETY: SignalSet::set_mask calls pthread_sigmask, which is an async signal safe function
        unsafe {
            cmd.pre_exec(move || {
                old_sigmask.set_mask()?;

                Ok(())
            })
        }
    }
}
