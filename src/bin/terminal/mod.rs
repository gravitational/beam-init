use std::io;
use std::os::fd::BorrowedFd;

pub(super) fn manage(pty: BorrowedFd<'_>) -> io::Result<()> {
    let mut stream = std::fs::File::from(pty.try_clone_to_owned()?);
    let _ = std::io::copy(&mut stream, &mut std::io::stdout());

    Ok(())
}
