use std::ffi::c_int;
use std::io;
use std::ops::ControlFlow;

use beam_init::system::signalfd::SignalFd;
use libc::signalfd_siginfo;
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;

use beam_init::system::signal_set::SignalSet;

pub fn init<Fut: Future<Output = ControlFlow<()>> + Send>(
    signals: &[c_int],
    mut callback: impl FnMut(signalfd_siginfo) -> Fut + Send + 'static,
) -> io::Result<OldSigmask> {
    let signal_set = SignalSet::new(signals)?;
    let mut rx = AsyncFd::new(SignalFd::new(&signal_set)?)?;
    let old_sigmask = signal_set.block()?;

    tokio::spawn(async move {
        loop {
            let siginfo = rx
                .async_io_mut(Interest::READABLE, |inner| inner.read())
                .await
                .expect("failed to read signal from signalfd");
            match callback(siginfo).await {
                ControlFlow::Continue(()) => {}
                ControlFlow::Break(()) => return,
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
