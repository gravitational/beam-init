use std::ffi::c_int;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::{io, mem, ptr};

use libc::{CLD_EXITED, CLD_KILLED, P_PIDFD, WEXITED, pid_t, siginfo_t};
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;

use crate::system::cerr;

#[derive(Debug)]
pub(crate) struct Pidfd {
    pid: pid_t,
    fd: OwnedFd,
}

impl Pidfd {
    pub(crate) fn for_pid(pid: pid_t) -> io::Result<Self> {
        let fd = cerr(unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as c_int)?;
        // SAFETY: SYS_pidfd_open returns either a valid pidfd or an error
        Ok(Pidfd {
            pid,
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }

    pub(crate) fn pid(&self) -> pid_t {
        self.pid
    }

    pub(crate) async fn wait(self) -> io::Result<ExitStatus> {
        let fd = AsyncFd::with_interest(self.fd, Interest::READABLE)?;
        let fd = fd.readable().await?.get_inner();
        let mut siginfo: libc::siginfo_t = unsafe { mem::zeroed() };
        unsafe {
            libc::waitid(
                P_PIDFD,
                fd.as_raw_fd().cast_unsigned(),
                &mut siginfo,
                WEXITED,
            )
        };

        // FIXME take si_code into account
        Ok(ExitStatus::from_raw(unsafe {
            match siginfo.si_code {
                CLD_EXITED => siginfo.si_status() << 8,
                CLD_KILLED => siginfo.si_status(),
                _ => unreachable!(),
            }
        }))
    }

    pub(crate) fn send_signal(&mut self, signal: c_int) -> io::Result<()> {
        cerr(unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.fd.as_raw_fd(),
                signal,
                ptr::null_mut::<siginfo_t>(),
                0,
            )
        } as c_int)?;
        Ok(())
    }

    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        Ok(Pidfd {
            pid: self.pid,
            fd: self.fd.try_clone()?,
        })
    }
}
