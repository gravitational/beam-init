use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ffi::{CStr, CString, NulError, OsString, c_char, c_uint};
use std::io::{self, PipeWriter, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::pin::pin;
use std::process::ExitStatus;
use std::ptr;
use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use futures_core::Stream;
use libc::{SIGCHLD, SIGCONT, SIGKILL, SIGSTOP, SIGTERM, WNOHANG, pid_t, signalfd_siginfo, uid_t};
use reqwest::StatusCode;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tokio_stream::StreamExt;

use crate::api_impl::Credentials;
use crate::fdstore::{FdStore, StoredFd};
use crate::logs::{AsyncRingBuffer, Logs};
use crate::signal_stream::OldSigmask;
use crate::{DEBUG_LOGS, Event};
use beam_init::system::fork::unsafe_fork;
use beam_init::system::pty::{Pty, PtyClient};
use beam_init::system::{
    _exit, cerr, close_range, getpid, kill_process_group, setpgid, setsid, waitpid,
};
use beam_init_api::Probe;

unsafe extern "C" {
    static mut environ: *mut *mut libc::c_char;
}

pub struct ServiceManager {
    old_sigmask: OldSigmask,
    services: BTreeMap<String, Service>,
    tx_event: mpsc::Sender<Event>,
    fdstore: FdStore,
}

#[derive(Debug)]
pub struct Service {
    pub config: ServiceConfig,
    pub state: ServiceState,
}

impl Service {
    /// Stop the liveness probe task for a service.
    fn abort_liveness_probe(&mut self) {
        if let Some(handle) = self.state.liveness_probe.take() {
            handle.abort();
        }
    }

    /// (Re)start the liveness probe task for a service.
    fn spawn_liveness_probe(&mut self, name: String, tx_event: mpsc::Sender<Event>) {
        let Some(probe) = self.config.liveness.clone() else {
            return;
        };

        let log_queue = Arc::clone(&self.state.logs.queue);

        let handle = tokio::spawn(run_liveness_probe(name, probe, tx_event, log_queue));
        self.state.liveness_probe = Some(handle.abort_handle());
    }
}

/// The configuration of a service.
///
/// This only changes when explicitly modified through the API.
#[derive(Debug)]
pub struct ServiceConfig {
    pub cmd: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub liveness: Option<Probe>,
    pub pty: bool,
    pub credentials: Credentials,
}

impl ServiceConfig {
    fn validate(&self) -> Result<(), ServiceError> {
        for (k, v) in &self.env {
            if k.is_empty() {
                return Err(ServiceError::InvalidRequest {
                    err: "environment keys may not be empty".to_string(),
                });
            }
            if k.contains('=') {
                return Err(ServiceError::InvalidRequest {
                    err: "environment keys may not contain '='".to_string(),
                });
            }
            if k.contains('\0') || v.contains('\0') {
                return Err(ServiceError::InvalidRequest {
                    err: "environment key/value may not contain NUL".to_string(),
                });
            }
        }
        Ok(())
    }
}

/// The runtime state of a service.
#[derive(Debug)]
pub struct ServiceState {
    pub status: ServiceStatus,
    pub logs: Logs,
    pub automatic_restart_attempts: u32,
    pub liveness_probe: Option<AbortHandle>,
}

#[derive(Debug)]
pub enum ServiceStatus {
    /// The service was stopped by the user or hasn't been started yet.
    Stopped,

    /// The service is currently running.
    Running {
        main_pid: pid_t,
        pty: Option<Pty<StoredFd>>,
    },

    /// The service is frozen (using SIGSTOP) but can be thawed (SIGCONT).
    Frozen {
        main_pid: pid_t,
        pty: Option<Pty<StoredFd>>,
    },

    /// The service was stopped, but will soon be started again as part of a restart.
    Restarting { main_pid: pid_t, name: String },

    /// The service has been requested to terminate and is in the process of shutting down.
    Stopping { main_pid: pid_t, prune: bool },

    /// The service exited with the given exit status.
    Exited(ExitStatus),

    /// The service failed to start with the given error.
    Error(io::Error),
}

#[derive(Debug)]
pub enum ServiceError {
    ServiceNotFound { name: String },
    ServiceExists { name: String },
    SpawnFailed { cmd: String, err: String },
    InvalidCredentials,
    InvalidRequest { err: String },
}

// FIXME serialize as json and deserialize and format error message inside the beamctl process?
impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        match self {
            ServiceError::ServiceNotFound { name } => (
                StatusCode::NOT_FOUND,
                format!("Service {name} was not found"),
            )
                .into_response(),
            ServiceError::ServiceExists { name } => (
                StatusCode::CONFLICT,
                format!("Service named `{name}` already exists"),
            )
                .into_response(),
            ServiceError::SpawnFailed { cmd, err } => (
                StatusCode::BAD_REQUEST,
                format!("Failed to spawn {cmd}: {err}"),
            )
                .into_response(),
            ServiceError::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "Service owned by a different user",
            )
                .into_response(),
            ServiceError::InvalidRequest { err } => (StatusCode::BAD_REQUEST, err).into_response(),
        }
    }
}

pub(crate) enum StartReason {
    /// The user requested that this service be (re)started.
    User,
    /// Beam-init requested that this service be (re)started (e.g. because it became unresponsive).
    Automatic,
}

impl ServiceManager {
    pub fn new(old_sigmask: OldSigmask, tx_event: mpsc::Sender<Event>, fdstore: FdStore) -> Self {
        ServiceManager {
            old_sigmask,
            services: BTreeMap::new(),
            tx_event,
            fdstore,
        }
    }

    pub fn handle_signal(&mut self, info: signalfd_siginfo) {
        if info.ssi_signo == SIGCHLD as u32 {
            loop {
                #[allow(
                    clippy::disallowed_methods,
                    reason = "this is the only place waitpid is ok"
                )]
                let (pid, status) = match waitpid(-1, WNOHANG) {
                    Ok((pid, status)) => (pid, status),
                    Err(err) if err.raw_os_error() == Some(libc::ECHILD) => {
                        // No more zombies to wait for. While the man page of wait/waitpid only
                        // explicitly says ECHILD happens for wait when there is no child to wait
                        // for, wait is implemented in terms of waitpid, so waitpid has to have the
                        // same behavior.
                        break;
                    }
                    Err(err) => panic!("waitpid failed with {err:?}"),
                };
                if pid == 0 {
                    return;
                }

                for (name, service) in self.services.iter_mut() {
                    match service.state.status {
                        ServiceStatus::Running { main_pid, .. } if main_pid == pid => {
                            service.state.status = ServiceStatus::Exited(status);
                            service.abort_liveness_probe();
                            break;
                        }
                        ServiceStatus::Stopping { main_pid, prune } if main_pid == pid => {
                            service.abort_liveness_probe();
                            if prune {
                                let name = name.clone();
                                self.services.remove(&name);
                            } else {
                                service.state.status = ServiceStatus::Stopped;
                            }

                            break;
                        }
                        ServiceStatus::Restarting { main_pid, ref name } if main_pid == pid => {
                            let name = name.clone();
                            service.abort_liveness_probe();
                            // start_service will set the service status to Error when an error occurs.
                            // There is nothing else we can do with an error here, so ignore it.
                            let credentials = service.config.credentials;
                            let _ = self.start_service(credentials, &name, StartReason::Automatic);
                            break;
                        }

                        _ => { /* ignore */ }
                    }
                }
            }
        }
    }

    pub async fn copy_logs(
        &self,
        credentials: Credentials,
        name: &str,
    ) -> Result<Vec<String>, ServiceError> {
        let service = self.get_service(credentials, name)?;

        Ok(service.state.logs.copy_logs().await)
    }

    pub fn log_reader(
        &self,
        credentials: Credentials,
        name: &str,
    ) -> Result<impl Stream<Item = String> + 'static, ServiceError> {
        let service = self.get_service(credentials, name)?;

        Ok(service.state.logs.new_reader())
    }

    pub fn create_service(
        &mut self,
        name: String,
        config: ServiceConfig,
    ) -> Result<(), ServiceError> {
        config.validate()?;

        let logs = Logs::new();

        if *DEBUG_LOGS {
            let reader = logs.new_reader();
            let name2 = name.clone();
            tokio::spawn(async move {
                let mut reader = pin!(reader);
                while let Some(line) = reader.next().await {
                    println!("[{name2}] {line}");
                }
            });
        }

        match self.services.entry(name.clone()) {
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(Service {
                    config,
                    state: ServiceState {
                        status: ServiceStatus::Stopped,
                        logs,
                        automatic_restart_attempts: 0,
                        liveness_probe: None,
                    },
                });
                Ok(())
            }
            Entry::Occupied(_) => Err(ServiceError::ServiceExists { name }),
        }
    }

    pub fn get_service(
        &self,
        credentials: Credentials,
        name: &str,
    ) -> Result<&Service, ServiceError> {
        let Some(service) = self.services.get(name) else {
            return Err(ServiceError::ServiceNotFound {
                name: name.to_owned(),
            });
        };

        if credentials.uid != 0 && credentials.uid != service.config.credentials.uid {
            return Err(ServiceError::InvalidCredentials);
        }

        Ok(service)
    }

    fn get_service_mut(
        &mut self,
        credentials: Credentials,
        name: &str,
    ) -> Result<&mut Service, ServiceError> {
        let Some(service) = self.services.get_mut(name) else {
            return Err(ServiceError::ServiceNotFound {
                name: name.to_owned(),
            });
        };

        if credentials.uid != 0 && credentials.uid != service.config.credentials.uid {
            return Err(ServiceError::InvalidCredentials);
        }

        Ok(service)
    }

    pub fn start_service(
        &mut self,
        credentials: Credentials,
        name: &str,
        reason: StartReason,
    ) -> Result<(), ServiceError> {
        let old_sigmask = self.old_sigmask;

        let tx_event = self.tx_event.clone();
        let service = self
            .services
            .get_mut(name)
            .ok_or_else(|| ServiceError::ServiceNotFound {
                name: name.to_owned(),
            })?;
        if credentials.uid != 0 && credentials.uid != service.config.credentials.uid {
            return Err(ServiceError::InvalidCredentials);
        }

        eprintln!("Starting service {name}");

        service.state.automatic_restart_attempts = match reason {
            StartReason::User => 0,
            StartReason::Automatic => service.state.automatic_restart_attempts.saturating_add(1),
        };

        let mut pty = service
            .config
            .pty
            .then(|| Pty::new(|fd| self.fdstore.add(fd, credentials.uid)))
            .transpose()
            .map_err(|err| {
                let err_str = err.to_string();
                println!("[{name}] Failed to create a pty: {err_str}");
                service.state.status = ServiceStatus::Error(err);
                ServiceError::SpawnFailed {
                    cmd: service.config.cmd.clone(),
                    err: err_str,
                }
            })?;

        let sink = if let Some(terminal) = &mut pty {
            add_single_log_message(
                &service.state.logs,
                format!("[process connected to pty: {}]", terminal.path.display()),
            );

            Sink::PTY(terminal.client())
        } else {
            let log_writer = service
                .state
                .logs
                .new_writer()
                .expect("failed to create log writer");

            Sink::Log(log_writer)
        };

        match spawn_service(old_sigmask, &service.config, sink) {
            Ok(child_pid) => {
                service.state.status = ServiceStatus::Running {
                    main_pid: child_pid,
                    pty,
                };
                service.spawn_liveness_probe(name.to_owned(), tx_event);
                Ok(())
            }
            Err(err) => {
                let err_str = err.to_string();
                if *DEBUG_LOGS {
                    eprintln!("[{name}] Failed to spawn: {err_str}");
                }
                service.state.status = ServiceStatus::Error(err);
                Err(ServiceError::SpawnFailed {
                    cmd: service.config.cmd.clone(),
                    err: err_str,
                })
            }
        }
    }

    pub fn freeze_service(
        &mut self,
        credentials: Credentials,
        name: &str,
    ) -> Result<(), ServiceError> {
        let service = self.get_service_mut(credentials, name)?;

        match service.state.status {
            ServiceStatus::Stopped
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Restarting { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_) => {
                // No process to freeze.
            }
            ServiceStatus::Frozen { .. } => {
                // This process is already frozen.
            }
            ServiceStatus::Running {
                main_pid,
                ref mut pty,
            } => {
                let pty = pty.take();
                service.abort_liveness_probe();
                kill_process_group(main_pid, SIGSTOP).expect("process to exist");
                service.state.status = ServiceStatus::Frozen { main_pid, pty };
            }
        }

        Ok(())
    }

    pub fn thaw_service(
        &mut self,
        credentials: Credentials,
        name: &str,
    ) -> Result<(), ServiceError> {
        let tx_event = self.tx_event.clone();
        let service = self.get_service_mut(credentials, name)?;

        match service.state.status {
            ServiceStatus::Stopped
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Restarting { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_) => {
                // No process to thaw.
            }
            ServiceStatus::Running { .. } => {
                // This process is already running.
            }
            ServiceStatus::Frozen {
                main_pid,
                ref mut pty,
            } => {
                let pty = pty.take();
                kill_process_group(main_pid, SIGCONT).expect("process to exist");
                service.state.status = ServiceStatus::Running { main_pid, pty };
                // Resume probing now that the process is running again.
                service.spawn_liveness_probe(name.to_owned(), tx_event)
            }
        }

        Ok(())
    }

    pub fn terminate_service(
        &mut self,
        credentials: Credentials,
        name: &str,
        prune: bool,
    ) -> Result<bool, ServiceError> {
        let service = self.get_service_mut(credentials, name)?;

        match service.state.status {
            ServiceStatus::Stopped | ServiceStatus::Exited(_) | ServiceStatus::Error(_) => {
                if prune {
                    self.services.remove(name);
                }

                // Stopped already.
                return Ok(true);
            }
            ServiceStatus::Stopping {
                main_pid,
                prune: old_prune,
            } => {
                service.state.status = ServiceStatus::Stopping {
                    main_pid,
                    prune: prune || old_prune,
                };
            }
            ServiceStatus::Running { main_pid, .. }
            | ServiceStatus::Frozen { main_pid, .. }
            | ServiceStatus::Restarting { main_pid, .. } => {
                service.abort_liveness_probe();
                service.state.status = ServiceStatus::Stopping { main_pid, prune };
                kill_process_group(main_pid, SIGTERM).expect("process to exist");
            }
        }

        Ok(false)
    }

    pub fn terminate_restart_service(
        &mut self,
        credentials: Credentials,
        name: &str,
    ) -> Result<(), ServiceError> {
        let service = self.get_service_mut(credentials, name)?;

        match service.state.status {
            ServiceStatus::Stopped
            | ServiceStatus::Restarting { .. }
            | ServiceStatus::Stopping { .. } => {
                // all good
            }
            ServiceStatus::Running { main_pid, .. } | ServiceStatus::Frozen { main_pid, .. } => {
                service.abort_liveness_probe();
                service.state.status = ServiceStatus::Restarting {
                    main_pid,
                    name: name.to_owned(),
                };
                kill_process_group(main_pid, SIGTERM).expect("process to exist");
            }
            ServiceStatus::Exited(_) | ServiceStatus::Error(_) => {
                // nothing to do
            }
        }

        Ok(())
    }

    pub fn kill_service(
        &mut self,
        credentials: Credentials,
        name: &str,
    ) -> Result<(), ServiceError> {
        let service = self.get_service_mut(credentials, name)?;

        match service.state.status {
            ServiceStatus::Stopped => {
                // all good
            }
            ServiceStatus::Running { .. } | ServiceStatus::Frozen { .. } => {
                panic!("service {name} was killed without being terminated")
            }
            ServiceStatus::Stopping { main_pid, prune: _ } => {
                kill_process_group(main_pid, SIGKILL).expect("process to exist");
            }
            ServiceStatus::Restarting { main_pid, .. } => {
                // Prevent the restart, only stop this service.
                service.state.status = ServiceStatus::Stopping {
                    main_pid,
                    prune: false,
                };

                kill_process_group(main_pid, SIGKILL).expect("process to exist");
            }
            ServiceStatus::Exited(_) | ServiceStatus::Error(_) => {
                // nothing to do
            }
        }

        Ok(())
    }

    pub fn kill_restart_service(
        &mut self,
        credentials: Credentials,
        name: &str,
    ) -> Result<(), ServiceError> {
        let service = self.get_service_mut(credentials, name)?;

        match service.state.status {
            ServiceStatus::Stopped => {
                // all good
            }
            ServiceStatus::Running { .. } | ServiceStatus::Frozen { .. } => {
                panic!("service {name} was killed without being terminated")
            }
            ServiceStatus::Stopping { main_pid, .. }
            | ServiceStatus::Restarting { main_pid, .. } => {
                kill_process_group(main_pid, SIGKILL).expect("process to exist");
            }
            ServiceStatus::Exited(_) | ServiceStatus::Error(_) => {
                // nothing to do
            }
        }

        Ok(())
    }

    pub fn list_services(&self) -> impl Iterator<Item = (&String, &ServiceStatus)> {
        self.services
            .iter()
            .map(|(name, service)| (name, &service.state.status))
    }
}

async fn run_liveness_probe(
    name: String,
    probe: Probe,
    tx_event: mpsc::Sender<Event>,
    logger: Arc<AsyncRingBuffer>,
) {
    tokio::time::sleep(probe.initial_delay).await;

    let client = reqwest::Client::new();
    let url = format!("http://localhost:{}{}", probe.port, probe.path);
    let mut consecutive_failures: usize = 0;

    loop {
        let healthy = match client.get(url.as_str()).timeout(probe.period).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        };

        if healthy {
            consecutive_failures = 0;
        } else {
            consecutive_failures += 1;
            logger
                .push(format!(
                    "[liveness probe failed ({consecutive_failures}/{})]",
                    probe.failure_threshold
                ))
                .await;

            if consecutive_failures > probe.max_retries {
                logger
                    .push(format!(
                        "[liveness probe exceeded max retries (max_retries={})]",
                        probe.max_retries
                    ))
                    .await;
                return;
            }

            if consecutive_failures >= probe.failure_threshold {
                logger
                    .push("[liveness probe exhausted. requesting restart]".to_owned())
                    .await;
                let _ = tx_event.send(Event::ProbeFailed { name }).await;
                return;
            }
        }

        tokio::time::sleep(probe.period).await;
    }
}

fn add_single_log_message(logs: &Logs, msg: String) {
    let queue = Arc::clone(&logs.queue);
    tokio::spawn(async move {
        queue.push(msg).await;
    });
}

#[allow(clippy::upper_case_acronyms)]
enum Sink<'a> {
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

fn spawn_service(old_sigmask: OldSigmask, config: &ServiceConfig, sink: Sink) -> io::Result<pid_t> {
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

    // FIXME(rcanderson23): this keeps the current behavior but we do not want user services to
    // inherit a copy of beam-init's environment so we should construct one
    let mut env: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    env.extend(
        config
            .env
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v))),
    );
    let envp_store = env
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
