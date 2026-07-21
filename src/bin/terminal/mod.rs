use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};

use crate::unix_socket::cerr;

pub(super) fn manage(pty: OwnedFd) -> io::Result<()> {
    let mut app_r = File::from(pty.try_clone()?);
    let mut app_w = File::from(pty);

    let mut source = std::io::stdin();
    let mut sink = std::io::stdout();

    let mut poller = mio::Poll::new()?;
    let reg = poller.registry();

    const CAN_READ_FROM_PTY: mio::Token = mio::Token(0);
    const CAN_READ_FROM_CONTROLLER: mio::Token = mio::Token(1);

    set_nonblocking(&source)?;
    set_nonblocking(&app_r)?;

    reg.register(
        &mut mio::unix::SourceFd(&source.as_raw_fd()),
        CAN_READ_FROM_CONTROLLER,
        mio::Interest::READABLE,
    )?;
    reg.register(
        &mut mio::unix::SourceFd(&app_r.as_raw_fd()),
        CAN_READ_FROM_PTY,
        mio::Interest::READABLE,
    )?;

    let mut events = mio::Events::with_capacity(1024);
    loop {
        poller.poll(&mut events, None)?;
        for event in &events {
            let res = match event.token() {
                CAN_READ_FROM_PTY => {
                    let res = std::io::copy(&mut app_r, &mut sink);
                    let _ = sink.flush();
                    res
                }
                CAN_READ_FROM_CONTROLLER => {
                    let res = std::io::copy(&mut source, &mut app_w);
                    let _ = app_w.flush();
                    res
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
