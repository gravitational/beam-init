pub(crate) struct DropBomb;

impl Drop for DropBomb {
    fn drop(&mut self) {
        std::process::abort();
    }
}

// This is a macro rather than function call to allow rustc to know that code
// running in the fork will never return and thus can drop pipes that the parent
// still needs to access later.
//
/// Fork our process. The passed block may not return. Returns `io::Result<pid_t>`.
///
/// # Safety
///
/// Unless the process is single threaded, only async-signal-safe functions may
/// be called inside the block.
macro_rules! unsafe_fork {
    ($f:block) => {
        match $crate::system::cerr(libc::fork()) {
            Ok(0) => {
                // Ensure we never unwind out of this function in the forked process
                let _bomb = $crate::system::fork::DropBomb;

                // Ensure that the provided block diverges.
                let _: std::convert::Infallible = { $f };
            }
            Ok(pid) => Ok(pid),
            Err(err) => Err(err),
        }
    };
}
pub(crate) use unsafe_fork;
