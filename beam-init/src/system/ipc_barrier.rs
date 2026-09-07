use std::io::{self, PipeReader, PipeWriter, Read};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

#[allow(dead_code)]
pub struct IpcBarrierNotify(PipeWriter);

pub struct IpcBarrierWaiter(PipeReader);

pub fn barrier() -> io::Result<(IpcBarrierWaiter, IpcBarrierNotify)> {
    let (rx, tx) = io::pipe()?;
    Ok((IpcBarrierWaiter(rx), IpcBarrierNotify(tx)))
}

impl IpcBarrierWaiter {
    pub fn wait(mut self) -> io::Result<()> {
        match self.0.read(&mut [0]) {
            Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Ok(0) => Ok(()),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "barrier pipe read returned data",
            )),
            Err(err) => Err(err),
        }
    }
}

impl From<OwnedFd> for IpcBarrierWaiter {
    fn from(fd: OwnedFd) -> Self {
        Self(PipeReader::from(fd))
    }
}

impl AsFd for IpcBarrierWaiter {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
