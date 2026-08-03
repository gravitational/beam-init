//! This module is modified from from sudo-rs's term/user_term.rs, which is itself a port of Todd Miller's sudo
//! lib/util/term.c with some changes to make it Rust-like. Copyright information:
//!
//! Copyright (c) 1994-1996, 1998-2026 Todd C. Miller <Todd.Miller@sudo.ws>
//! Copyright (c) 2025 Trifecta Tech Foundation and Contributors
//!
//! Permission to use, copy, modify, and distribute this software for any purpose with or without fee is hereby granted,
//! provided that the above copyright notice and this permission notice appear in all copies.

use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    mem::MaybeUninit,
    os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd},
};

use libc::{
    ECHO, ECHOCTL, ECHOE, ECHOK, ECHOKE, ECHONL, ICANON, ICRNL, IEXTEN, IGNCR, IGNPAR, IMAXBEL,
    INLCR, INPCK, ISTRIP, IUCLC, IUTF8, IXANY, IXOFF, IXON, NOFLSH, OCRNL, OLCUC, ONLCR, ONLRET,
    ONOCR, OPOST, PARMRK, PENDIN, TCSADRAIN, TCSAFLUSH, TIOCGWINSZ, TIOCSWINSZ, TOSTOP, XCASE,
    ioctl, tcflag_t, tcgetattr, tcsetattr, termios, winsize,
};

use beam_init::system::cerr;

const INPUT_FLAGS: tcflag_t = IGNPAR
    | PARMRK
    | INPCK
    | ISTRIP
    | INLCR
    | IGNCR
    | ICRNL
    | IUCLC
    | IXON
    | IXANY
    | IXOFF
    | IMAXBEL
    | IUTF8;
const OUTPUT_FLAGS: tcflag_t = OPOST | OLCUC | ONLCR | OCRNL | ONOCR | ONLRET;
const LOCAL_FLAGS: tcflag_t = ICANON
    | XCASE
    | ECHO
    | ECHOE
    | ECHOK
    | ECHONL
    | NOFLSH
    | TOSTOP
    | IEXTEN
    | ECHOCTL
    | ECHOKE
    | PENDIN;

/// Type to manipulate the settings of the user's terminal.
pub struct UserTerm {
    tty: File,
    original_termios: termios,
}

impl UserTerm {
    /// Open the user's terminal.
    pub(crate) fn open() -> io::Result<Self> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .and_then(Self::from)
    }

    pub(crate) fn from(tty: File) -> io::Result<Self> {
        let original_termios = get_termios(tty.as_raw_fd())?;

        Ok(Self {
            tty,
            original_termios,
        })
    }

    /// Synchronize settings of the provided fd to this terminal.
    /// - This will copy most settings of 'client' to self
    /// - But it will inform 'client' of the current window size
    pub(crate) fn sync<D: AsFd>(&mut self, client: &D) -> io::Result<()> {
        let client = client.as_fd().as_raw_fd();

        let mut tt_dst = get_termios(self.tty.as_raw_fd())?;

        // SAFETY: tt_src will be initialized by `tcgetattr`.
        let tt_src = unsafe {
            let mut tt_src = MaybeUninit::<termios>::uninit();
            cerr(tcgetattr(client, tt_src.as_mut_ptr()))?;
            tt_src.assume_init()
        };

        // Clear selected input, output, and local flags.
        tt_dst.c_iflag &= !INPUT_FLAGS;
        tt_dst.c_oflag &= !OUTPUT_FLAGS;
        tt_dst.c_lflag &= !LOCAL_FLAGS;

        // Copy selected input, output, and local flags.
        tt_dst.c_iflag |= tt_src.c_iflag & INPUT_FLAGS;
        tt_dst.c_oflag |= tt_src.c_oflag & OUTPUT_FLAGS;
        tt_dst.c_lflag |= tt_src.c_lflag & LOCAL_FLAGS;

        // SAFETY: dst is a valid file descriptor and `tt_dst` is an
        // initialized struct obtained through tcgetattr; so this is safe to
        // pass to `tcsetattr`.
        cerr(unsafe { tcsetattr(self.as_raw_fd(), TCSAFLUSH, &tt_dst) })?;

        // Transfer the window size from self.tty to client
        let mut wsize = MaybeUninit::<winsize>::uninit();
        // SAFETY: TIOCGWINSZ ioctl expects one argument of type *mut winsize
        cerr(unsafe { ioctl(self.as_raw_fd(), TIOCGWINSZ, wsize.as_mut_ptr()) })?;
        // SAFETY: wsize has been initialized by the TIOCGWINSZ ioctl
        cerr(unsafe { ioctl(client, TIOCSWINSZ, wsize.as_ptr()) })?;

        Ok(())
    }

    /// Restore the saved terminal settings if we are in the foreground process group.
    ///
    /// This change is done after waiting for all the queued output to be written. To discard the
    /// queued input `flush` must be set to `true`.
    fn restore(&mut self, flush: bool) -> io::Result<()> {
        let fd = self.tty.as_raw_fd();
        let flags = if flush { TCSAFLUSH } else { TCSADRAIN };
        // SAFETY: `fd` is a valid file descriptor for the tty; and `termios` is a valid pointer
        // that was obtained through `tcgetattr`.
        cerr(unsafe { tcsetattr(fd, flags, &self.original_termios) })?;

        Ok(())
    }
}

/// Retrieve the current settings to be able to restore later
fn get_termios(fd: RawFd) -> io::Result<termios> {
    // SAFETY: `termios` is a valid pointer to pass to tcgetattr; if that calls succeeds,
    // it will have initialized the `termios` structure
    Ok(unsafe {
        let mut termios = MaybeUninit::uninit();
        cerr(tcgetattr(fd, termios.as_mut_ptr()))?;
        termios.assume_init()
    })
}

impl AsFd for UserTerm {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.tty.as_fd()
    }
}

impl Read for UserTerm {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.tty.read(buf)
    }
}

impl Write for UserTerm {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tty.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tty.flush()
    }
}

impl AsRawFd for UserTerm {
    fn as_raw_fd(&self) -> RawFd {
        self.as_fd().as_raw_fd()
    }
}

impl Drop for UserTerm {
    fn drop(&mut self) {
        self.restore(true).expect("to restore terminal settings");
    }
}
