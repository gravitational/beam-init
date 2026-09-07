use std::ffi::{CStr, CString, NulError, c_char, c_uint};
use std::io::{self, PipeWriter, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::ptr;

use libc::{pid_t, uid_t};

use crate::api_impl::Credentials;
use crate::fdstore::StoredFd;
use crate::services::ServiceConfig;
use crate::signal_stream::OldSigmask;
use beam_init::system::fork::unsafe_fork;
use beam_init::system::pty::PtyClient;
use beam_init::system::{_exit, cerr, close_range, getpid, setpgid, setsid};

unsafe extern "C" {
    static mut environ: *mut *mut libc::c_char;
}

#[allow(clippy::upper_case_acronyms)]
pub(crate) enum Sink<'a> {
    Log(OwnedFd),
    PTY(PtyClient<'a, StoredFd>),
}

fn expect_no_panic<T>(res: io::Result<T>, msg: &'static str) -> T {
    match res {
        Ok(x) => x,
        Err(err) => {
            eprintln!("{msg}: {err}");
            _exit(101);
        }
    }
}

pub(crate) fn spawn_service(
    old_sigmask: OldSigmask,
    config: &ServiceConfig,
    sink: Sink,
) -> io::Result<pid_t> {
    let cmd = CString::new(config.cmd.clone())?;

    let args = config
        .args
        .iter()
        .map(|arg| CString::new(arg.to_owned()))
        .collect::<Result<Vec<_>, NulError>>()?;
    let args = Some(cmd.as_ptr())
        .into_iter()
        .chain(args.iter().map(|arg| arg.as_ptr()))
        .chain(Some(ptr::null()))
        .collect::<Vec<_>>();

    let envp_store = config
        .env
        .iter()
        .map(|(k, v)| {
            let mut entry = k.clone().into_vec();
            entry.push(b'=');
            entry.extend_from_slice(v.as_bytes());
            CString::new(entry)
        })
        .collect::<Result<Vec<_>, NulError>>()?;
    let mut envp = envp_store
        .iter()
        .map(|p| p.as_ptr().cast_mut())
        .chain(Some(ptr::null_mut()))
        .collect::<Box<[_]>>();

    let (mut err_rx, err_tx) = io::pipe()?;
    let (mut pid_rx, mut pid_tx) = io::pipe()?;
    // SAFETY: We only run async-signal-safe functions inside the child process.
    unsafe {
        unsafe_fork!({
            expect_no_panic(old_sigmask.restore_sigmask(), "failed to restore sigmask");

            // Create a new session and process group led by this process.
            // Uses the current PID as the PGID of the new process group.
            // Using only a new process group won't work as then bash will
            // hang if the container has a tty attached.
            expect_no_panic(setsid(), "failed to setsid");

            let has_ctty = matches!(sink, Sink::PTY(_));
            sink.set_stdioe(config.credentials.uid);

            if !has_ctty {
                // Double fork to ensure the service can't accidentally attach a
                // controlling tty to the session when opening a file that happens
                // to be a tty.

                let service_pid = expect_no_panic(
                    unsafe_fork!({
                        // Create a new process group led by this process.
                        // Uses the current PID as the PGID of the new process group.
                        expect_no_panic(setpgid(0, 0), "failed to `setpgid`");

                        // SAFETY: args is a NULL terminated list of C strings.
                        exec_with_creds_and_err_pipe(
                            &cmd,
                            &args,
                            &mut envp,
                            &config.credentials,
                            err_tx,
                        )
                    }),
                    "failed to fork",
                );
                drop(err_tx);

                expect_no_panic(
                    pid_tx.write_all(&pid_t::to_ne_bytes(service_pid)),
                    "failed to write pid",
                );

                _exit(0);
            } else {
                // Avoid a double fork for now when a pty is attached as the
                // session leader exiting causes a SIGHUP which will kill the
                // child if it happened after the exec.
                // FIXME add a persistent monitor process

                let service_pid = getpid();
                expect_no_panic(
                    pid_tx.write_all(&pid_t::to_ne_bytes(service_pid)),
                    "failed to write pid",
                );

                // SAFETY: args is a NULL terminated list of C strings.
                exec_with_creds_and_err_pipe(&cmd, &args, &mut envp, &config.credentials, err_tx)
            }
        })?
    };
    drop(err_tx);

    let mut err = [0; size_of::<i32>()];
    match err_rx.read_exact(&mut err) {
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
            let mut child_pid = [0; size_of::<pid_t>()];
            pid_rx.read_exact(&mut child_pid)?;
            Ok(pid_t::from_ne_bytes(child_pid))
        }
        Ok(()) => Err(io::Error::from_raw_os_error(i32::from_ne_bytes(err))),
        Err(err) => Err(err),
    }
}

impl Sink<'_> {
    fn set_stdioe(self, uid: uid_t) {
        match self {
            Sink::PTY(pty) => {
                let pty_fd = expect_no_panic(
                    pty.make_tty(uid),
                    "could not make the pty the controlling terminal",
                );

                // Set the pseudoterminal as stdin, stdout and stderr
                expect_no_panic(
                    [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO]
                        .into_iter()
                        .try_for_each(|fd| {
                            // SAFETY: dup2 is memory safe to call. This technically violates
                            // IO-safety, but nothing accessed after this point depends on
                            // stdout/stderr pointing to a particular fd.
                            cerr(unsafe { libc::dup2(pty_fd.as_raw_fd(), fd) })?;
                            Ok(())
                        }),
                    "failed to attach pty",
                );
            }
            Sink::Log(log_writer) => {
                // Set the log pipe as stdout and stderr
                expect_no_panic(
                    // SAFETY: as above
                    cerr(unsafe { libc::dup2(log_writer.as_raw_fd(), libc::STDOUT_FILENO) }),
                    "failed to set stdout",
                );
                expect_no_panic(
                    // SAFETY: as above
                    cerr(unsafe { libc::dup2(log_writer.as_raw_fd(), libc::STDERR_FILENO) }),
                    "failed to set stderr",
                );
            }
        }
    }
}

/// # Safety
///
/// `args` must be a NULL terminated list of C strings.
/// `envp` must be a NULL terminated list of C strings.
unsafe fn exec_with_creds_and_err_pipe(
    cmd: &CStr,
    args: &[*const c_char],
    envp: &mut [*mut c_char],
    credentials: &Credentials,
    mut err_tx: PipeWriter,
) -> ! {
    // Using raw syscall as musl doesn't have a close_range() wrapper.
    expect_no_panic(
        close_range(3, c_uint::MAX, libc::CLOSE_RANGE_CLOEXEC.cast_signed()),
        "failed to `close_range",
    );

    // Set the group and user ID (derived from socket) as well as an empty
    // supplementary group list.
    expect_no_panic(credentials.set_creds(), "failed to set process credentials");

    // SAFETY: Per the safety requirements of this function, args and env is a NULL
    // terminated lists of C strings.
    unsafe {
        environ = envp.as_mut_ptr();
        libc::execvp(cmd.as_ptr(), args.as_ptr())
    };

    // If we reach this point, the exec failed.
    let Some(err) = io::Error::last_os_error().raw_os_error() else {
        eprintln!("last_os_error didn't return OS error");
        _exit(101);
    };

    expect_no_panic(
        err_tx.write_all(&i32::to_ne_bytes(err)),
        "failed to write error code",
    );
    _exit(1);
}
