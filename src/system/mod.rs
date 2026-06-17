use std::ffi::c_int;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::{io, process};

use libc::pid_t;

use crate::system::pidfd::Pidfd;

pub mod fork;
pub mod pidfd;
pub mod signal_set;

pub fn cerr(retval: c_int) -> io::Result<c_int> {
    if retval == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(retval)
}

pub fn waitpid(pid: pid_t, options: c_int) -> io::Result<(pid_t, ExitStatus)> {
    let mut status = 0;
    // SAFETY: A valid mutable pointer is passed as status argument.
    let pid = cerr(unsafe { libc::waitpid(pid, &mut status, options) })?;
    Ok((pid, ExitStatus::from_raw(status)))
}

pub fn kill_process(pidfd: &mut Pidfd, sig: c_int) -> io::Result<()> {
    pidfd.send_signal(sig)
}

pub fn kill_process_group(pgid: pid_t, sig: c_int) -> io::Result<i32> {
    assert!(pgid > 1, "process group {pgid} is not valid");

    // SAFETY: kill won't cause UB for a nonexistent PID or invalid signal.
    cerr(unsafe { libc::kill(-pgid, sig) })
}

pub fn _exit(code: c_int) -> ! {
    // SAFETY: _exit is safe to call
    unsafe { libc::_exit(code) };
}

pub fn exit_with_signal(sig: c_int) -> ! {
    // SAFETY: This is always safe
    unsafe { libc::raise(sig) };
    process::abort();
}
