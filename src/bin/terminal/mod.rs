use std::fs::File;
use std::io::{self, Read, Write};
use std::mem;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};

use crate::unix_socket::cerr;
use signal_set::SignalSet;

#[path = "../../system/signal_set.rs"]
mod signal_set;

pub(super) fn manage(pid: libc::pid_t, pty: OwnedFd) -> io::Result<()> {
    let mut app = File::from(pty);

    let mut tty = File::options().read(true).write(true).open("/dev/tty")?;

    let mut signals = SignalFd::new(&[libc::SIGINT, libc::SIGQUIT, libc::SIGTSTP])?;

    let mut poller = mio::Poll::new()?;
    let reg = poller.registry();

    const CAN_READ_FROM_PTY: mio::Token = mio::Token(0);
    const CAN_READ_FROM_CONTROLLER: mio::Token = mio::Token(1);
    const SIGNAL_ARRIVED: mio::Token = mio::Token(2);

    set_nonblocking(&tty)?;
    set_nonblocking(&app)?;

    reg.register(
        &mut mio::unix::SourceFd(&tty.as_raw_fd()),
        CAN_READ_FROM_CONTROLLER,
        mio::Interest::READABLE,
    )?;
    reg.register(
        &mut mio::unix::SourceFd(&app.as_raw_fd()),
        CAN_READ_FROM_PTY,
        mio::Interest::READABLE,
    )?;
    reg.register(
        &mut mio::unix::SourceFd(&signals.as_raw_fd()),
        SIGNAL_ARRIVED,
        mio::Interest::READABLE,
    )?;

    let mut events = mio::Events::with_capacity(1024);
    loop {
        poller.poll(&mut events, None)?;
        for event in &events {
            let res = match event.token() {
                CAN_READ_FROM_PTY => {
                    let res = std::io::copy(&mut app, &mut tty);
                    let _ = tty.flush();
                    res
                }
                CAN_READ_FROM_CONTROLLER => {
                    let res = dbg!(std::io::copy(&mut tty, &mut app));
                    let _ = app.flush();
                    res
                }
                SIGNAL_ARRIVED => {
                    match signals.read()? {
                        sig @ (libc::SIGINT | libc::SIGQUIT) => {
                            //SAFETY: killpg is safe to call
                            unsafe {
                                libc::kill(pid, sig);
                            }
                        }
                        libc::SIGTSTP => {
                            // Suspend was received, detach
                            // TODO: this should also reset the terminal, actually, but for now this suffices
                            println!();
                            return Ok(());
                        }
                        _ => unreachable!("An unexpected signal was caught"),
                    }
                    continue;
                }
                _ => continue,
            };

            if terminated(res)? {
                return Ok(());
            }
        }
    }
}

fn set_nonblocking(fd: &impl AsFd) -> io::Result<()> {
    let raw_fd = fd.as_fd().as_raw_fd();

    //SAFETY: see man fcntl(2): it is passed a correct fd (since we lean on the
    //guarantees a type that implements AsFd must have), and the calls for F_GETFL and F_SETFL
    //follow the correct forms.
    unsafe {
        let flags = cerr(libc::fcntl(raw_fd, libc::F_GETFL))?;
        cerr(libc::fcntl(raw_fd, libc::F_SETFL, flags | libc::O_NONBLOCK))?;
    }

    Ok(())
}

fn terminated<T>(result: io::Result<T>) -> io::Result<bool> {
    match result {
        Ok(_) => Ok(true),
        Err(err) => {
            if err.raw_os_error() == Some(libc::EIO) {
                Ok(true)
            } else if err.kind() == io::ErrorKind::WouldBlock {
                Ok(false)
            } else {
                Err(err)
            }
        }
    }
}

pub struct SignalFd(File, SignalSet);

impl SignalFd {
    pub fn new(signals: &[libc::c_int]) -> io::Result<SignalFd> {
        let mut signal_set = SignalSet::empty()?;
        for &signum in signals {
            signal_set.add(signum)?;
        }

        use libc::{SFD_CLOEXEC, SFD_NONBLOCK};

        // -1 indicates creating a new signalfd receiving the given signals.
        // SAFETY: `signalfd` is passed a valid signal set pointer and returns an owned fd.
        let fd = unsafe {
            OwnedFd::from_raw_fd(cerr(libc::signalfd(
                -1,
                signal_set.as_ref(),
                SFD_CLOEXEC | SFD_NONBLOCK,
            ))?)
        };

        let file = File::from(fd);

        let old_sigmask = signal_set.block()?;

        Ok(SignalFd(file, old_sigmask))
    }

    pub fn read(&mut self) -> io::Result<libc::c_int> {
        let mut siginfo = [0; size_of::<libc::signalfd_siginfo>()];
        self.0.read_exact(&mut siginfo)?;
        // SAFETY: `signalfd_siginfo` does not contain any padding or
        // pointers, nor does `[u8; _]`. And `signalfd_siginfo` doesn't
        // have any private fields with invariants.
        let info = unsafe { mem::transmute::<[u8; _], libc::signalfd_siginfo>(siginfo) };
        Ok(info.ssi_signo.try_into().expect("signo to fit in c_int"))
    }
}

impl AsRawFd for SignalFd {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl Drop for SignalFd {
    fn drop(&mut self) {
        self.1.set_mask().expect("to restore signals");
    }
}
