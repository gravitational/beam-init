use std::fs::File;
use std::io;
use std::os::fd::{AsFd, AsRawFd, OwnedFd, RawFd};

use beam_init::system::signalfd::SignalFd;
use beam_init::system::{cerr, kill_process_group, signal_set::SignalSet};

mod user_term;
use user_term::UserTerm;

pub(super) fn manage(pid: libc::pid_t, pty: OwnedFd) -> io::Result<()> {
    let mut app = File::from(pty);

    let mut tty = UserTerm::open()?;
    tty.sync(&app)?;

    // send SIGWINCH to the application to stimulate it to redraw
    if let Err(err) = kill_process_group(pid, libc::SIGWINCH) {
        if err.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(err);
    }

    let mut signals =
        SignalFdRestore::new(&[libc::SIGINT, libc::SIGQUIT, libc::SIGTSTP, libc::SIGWINCH])?;

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
        tty.sync(&app)?;
        poller.poll(&mut events, None)?;
        for event in &events {
            let res = match event.token() {
                CAN_READ_FROM_PTY => std::io::copy(&mut app, &mut tty),
                CAN_READ_FROM_CONTROLLER => std::io::copy(&mut tty, &mut app),
                SIGNAL_ARRIVED => {
                    match signals.read()? {
                        sig @ (libc::SIGINT | libc::SIGQUIT | libc::SIGWINCH) => {
                            kill_process_group(pid, sig)?;
                            continue;
                        }
                        libc::SIGTSTP => {
                            // FIXME: send process to the background
                            // Suspend was received, detach
                            Ok(0)
                        }
                        _ => unreachable!("An unexpected signal was caught"),
                    }
                }
                _ => continue,
            };

            if terminated(res)? {
                // TODO: this should also reset the terminal, actually, but for now this suffices
                println!();
                return Ok(());
            }
        }
    }
}

fn set_nonblocking(fd: &impl AsFd) -> io::Result<()> {
    let raw_fd = fd.as_fd().as_raw_fd();

    // SAFETY: see man fcntl(2): it is passed a correct fd (since we lean on the
    // guarantees a type that implements AsFd must have), and the calls for F_GETFL and F_SETFL
    // follow the correct forms.
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

pub struct SignalFdRestore(SignalFd, SignalSet);

impl SignalFdRestore {
    pub fn new(signals: &[libc::c_int]) -> io::Result<SignalFdRestore> {
        let signal_set = SignalSet::new(signals)?;
        let file = SignalFd::new(&signal_set)?;
        let old_sigmask = signal_set.block()?;
        Ok(SignalFdRestore(file, old_sigmask))
    }

    pub fn read(&mut self) -> io::Result<libc::c_int> {
        let info = self.0.read()?;
        Ok(info.ssi_signo.try_into().expect("signo to fit in c_int"))
    }
}

impl AsRawFd for SignalFdRestore {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl Drop for SignalFdRestore {
    fn drop(&mut self) {
        self.1.set_mask().expect("to restore signals");
    }
}
