use std::ffi::c_int;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use libc::pid_t;

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
