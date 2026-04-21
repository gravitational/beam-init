use std::ffi::c_int;
use std::io;

pub fn cerr(retval: c_int) -> io::Result<c_int> {
    if retval == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(retval)
}
