use std::ffi::{c_int, c_uint};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::{io, process};

use libc::pid_t;

pub mod fork;
pub mod pty;
pub mod signal_set;
pub mod unix_socket;

pub fn cerr(retval: c_int) -> io::Result<c_int> {
    if retval == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(retval)
}

pub fn waitpid(pid: pid_t, options: c_int) -> io::Result<(pid_t, ExitStatus)> {
    let mut status = 0;
    #[allow(
        clippy::disallowed_methods,
        reason = "wrapper for libc::waitpid, itself marked as disallowed"
    )]
    // SAFETY: A valid mutable pointer is passed as status argument.
    let pid = cerr(unsafe { libc::waitpid(pid, &mut status, options) })?;
    Ok((pid, ExitStatus::from_raw(status)))
}

pub fn kill_process_group(pgid: pid_t, sig: c_int) -> io::Result<i32> {
    assert!(pgid > 1, "process group {pgid} is not valid");

    // SAFETY: kill won't cause UB for a nonexistent PID or invalid signal.
    match cerr(unsafe { libc::kill(-pgid, sig) }) {
        Err(e) if e.raw_os_error() == Some(libc::ESRCH) => {
            // The process moved to another process group, only kill the single process.
            // SAFETY: kill won't cause UB for a nonexistent PID or invalid signal.
            cerr(unsafe { libc::kill(pgid, sig) })
        }
        other => other,
    }
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

pub fn getpid() -> pid_t {
    // SAFETY: getpid is safe to call.
    unsafe { libc::getpid() }
}

pub fn setsid() -> io::Result<pid_t> {
    // SAFETY: setsid is safe to call.
    cerr(unsafe { libc::setsid() })
}

pub fn setpgid(pid: pid_t, pgid: pid_t) -> io::Result<()> {
    // SAFETY: setpgid is safe to call.
    cerr(unsafe { libc::setpgid(pid, pgid) }).map(|_| ())
}

pub fn close_range(first: c_uint, last: c_uint, flags: c_int) -> io::Result<()> {
    // SAFETY: SYS_close_range with CLOSE_RANGE_CLOEXEC doesn't violate IO safety.
    cerr(unsafe { libc::syscall(libc::SYS_close_range, first, last, flags) as c_int }).map(|_| ())
}
