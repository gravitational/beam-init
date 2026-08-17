use std::ffi::c_int;
use std::io;

use beam_init::system::signalfd::SignalFd;
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;

use crate::Event;
use beam_init::system::signal_set::SignalSet;

pub fn init(signals: &[c_int], tx_event: mpsc::Sender<Event>) -> io::Result<OldSigmask> {
    let mut signal_set = SignalSet::empty()?;
    for &signum in signals {
        signal_set.add(signum)?;
    }

    let mut rx = AsyncFd::new(SignalFd::new(&signal_set)?)?;

    let old_sigmask = signal_set.block()?;

    tokio::spawn(async move {
        loop {
            let siginfo = rx
                .async_io_mut(Interest::READABLE, |inner| inner.read())
                .await
                .expect("failed to read signal from signalfd");
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
    pub fn restore_sigmask(&self) -> io::Result<()> {
        self.0.set_mask()?;
        Ok(())
    }
}
