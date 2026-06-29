use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ffi::{CString, NulError};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::pin::pin;
use std::process::ExitStatus;
use std::ptr;

use axum::response::{IntoResponse, Response};
use futures_core::Stream;
use libc::{SIGCHLD, SIGCONT, SIGKILL, SIGSTOP, SIGTERM, WNOHANG, pid_t, signalfd_siginfo};
use reqwest::StatusCode;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tokio_stream::StreamExt;

use crate::Event;
use crate::logs::Logs;
use crate::signal_stream::OldSigmask;
use crate::system::fork::unsafe_fork;
use crate::system::{_exit, cerr, kill_process_group, waitpid};
use beam_init::api::Probe;

pub struct ServiceManager {
    old_sigmask: OldSigmask,
    services: BTreeMap<String, Service>,
    tx_event: mpsc::Sender<Event>,
}

#[derive(Debug)]
pub struct Service {
    pub config: ServiceConfig,
    pub state: ServiceState,
}

impl Service {
    /// Stop the readiness probe task for a service.
    fn abort_readiness_probe(&mut self) {
        if let Some(handle) = self.state.readiness_probe.take() {
            handle.abort();
        }
    }

    /// (Re)start the readiness probe task for a service.
    fn spawn_readiness_probe(&mut self, name: String, tx_event: mpsc::Sender<Event>) {
        let Some(probe) = self.config.readiness.clone() else {
            return;
        };

        let handle = tokio::spawn(run_readiness_probe(name, probe, tx_event));
        self.state.readiness_probe = Some(handle.abort_handle());
    }
}

/// The configuration of a service.
///
/// This only changes when explicitly modified through the API.
#[derive(Debug)]
pub struct ServiceConfig {
    pub cmd: String,
    pub args: Vec<String>,
    pub readiness: Option<Probe>,
}

/// The runtime state of a service.
#[derive(Debug)]
pub struct ServiceState {
    pub status: ServiceStatus,
    pub logs: Logs,
    pub automatic_restart_attempts: u32,
    pub readiness_probe: Option<AbortHandle>,
}

#[derive(Debug)]
pub enum ServiceStatus {
    /// The service was stopped by the user or hasn't been started yet.
    Stopped { reason: StopReason },

    /// The service is currently running.
    Running { main_pid: u32 },

    /// The service is frozen (using SIGSTOP) but can be thawed (SIGCONT).
    Frozen { main_pid: u32 },

    /// The service has been requested to terminate and is in the process of shutting down.
    Stopping { main_pid: u32, reason: StopReason },

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
        }
    }
}

pub(crate) enum StartReason {
    /// The user requested that this service be (re)started.
    User,
    /// Beam-init requested that this service be (re)started (e.g. because it became unresponsive).
    Automatic,
}

#[derive(Debug, Copy, Clone)]
pub enum StopReason {
    /// The user requested that this service be stopped.
    User,
    /// Beam-init requested that this service be stopped (e.g. as part of a restart).
    Automatic,
}

impl ServiceManager {
    pub fn new(old_sigmask: OldSigmask, tx_event: mpsc::Sender<Event>) -> Self {
        ServiceManager {
            old_sigmask,
            services: BTreeMap::new(),
            tx_event,
        }
    }

    pub fn handle_signal(&mut self, info: signalfd_siginfo) {
        dbg!(info.ssi_signo);
        if info.ssi_signo == SIGCHLD as u32 {
            let (pid, status) = waitpid(info.ssi_pid as pid_t, WNOHANG).expect(
                "got SIGCHLD for non-existent process. maybe there is an incorrect waitpid elsewhere?",
            );
            if pid == 0 {
                return;
            }

            for service in self.services.values_mut() {
                match service.state.status {
                    ServiceStatus::Running { main_pid } if main_pid == info.ssi_pid => {
                        service.state.status = ServiceStatus::Exited(status);
                        service.abort_readiness_probe();
                        return;
                    }
                    ServiceStatus::Stopping { main_pid, reason } if main_pid == info.ssi_pid => {
                        service.state.status = ServiceStatus::Stopped { reason };
                        service.abort_readiness_probe();
                        return;
                    }
                    _ => { /* ignore */ }
                }
            }
        }
    }

    pub async fn copy_logs(&self, name: &str) -> Result<Vec<String>, ServiceError> {
        let service = self.get_service(name)?;

        Ok(service.state.logs.copy_logs().await)
    }

    pub fn log_reader(
        &self,
        name: &str,
    ) -> Result<impl Stream<Item = String> + 'static, ServiceError> {
        let service = self.get_service(name)?;

        Ok(service.state.logs.new_reader())
    }

    pub fn create_service(
        &mut self,
        name: String,
        config: ServiceConfig,
    ) -> Result<(), ServiceError> {
        let logs = Logs::new();

        let reader = logs.new_reader();
        let name2 = name.clone();
        tokio::spawn(async move {
            let mut reader = pin!(reader);
            while let Some(line) = reader.next().await {
                println!("[{name2}] {line}");
            }
        });

        match self.services.entry(name.clone()) {
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(Service {
                    config,
                    state: ServiceState {
                        status: ServiceStatus::Stopped {
                            reason: StopReason::User,
                        },
                        logs,
                        automatic_restart_attempts: 0,
                        readiness_probe: None,
                    },
                });
                Ok(())
            }
            Entry::Occupied(_) => Err(ServiceError::ServiceExists { name }),
        }
    }

    pub fn try_get_service(&self, name: &str) -> Option<&Service> {
        self.services.get(name)
    }

    pub fn get_service(&self, name: &str) -> Result<&Service, ServiceError> {
        self.services
            .get(name)
            .ok_or_else(|| ServiceError::ServiceNotFound {
                name: name.to_owned(),
            })
    }

    fn get_service_mut(&mut self, name: &str) -> Result<&mut Service, ServiceError> {
        self.services
            .get_mut(name)
            .ok_or_else(|| ServiceError::ServiceNotFound {
                name: name.to_owned(),
            })
    }

    pub fn start_service(&mut self, name: &str, reason: StartReason) -> Result<(), ServiceError> {
        let old_sigmask = self.old_sigmask;

        let tx_event = self.tx_event.clone();
        let service = self.get_service_mut(name)?;

        println!("Starting service {name}");

        let log_writer = service
            .state
            .logs
            .new_writer()
            .expect("failed to create log writer");

        service.state.automatic_restart_attempts = match reason {
            StartReason::User => 0,
            StartReason::Automatic => service.state.automatic_restart_attempts.saturating_add(1),
        };

        match spawn_service(old_sigmask, &service.config, log_writer) {
            Ok(child_pid) => {
                service.state.status = ServiceStatus::Running {
                    main_pid: child_pid as u32,
                };
                service.spawn_readiness_probe(name.to_owned(), tx_event);
                Ok(())
            }
            Err(err) => {
                let err_str = err.to_string();
                println!("[{name}] Failed to spawn: {err_str}");
                service.state.status = ServiceStatus::Error(err);
                Err(ServiceError::SpawnFailed {
                    cmd: service.config.cmd.clone(),
                    err: err_str,
                })
            }
        }
    }

    pub fn freeze_service(&mut self, name: &str) -> Result<(), ServiceError> {
        let service = self.get_service_mut(name)?;

        match service.state.status {
            ServiceStatus::Stopped { .. }
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_) => {
                // No process to freeze.
            }
            ServiceStatus::Frozen { .. } => {
                // This process is already frozen.
            }
            ServiceStatus::Running { main_pid } => {
                service.abort_readiness_probe();
                kill_process_group(main_pid as pid_t, SIGSTOP).expect("process to exist");
                service.state.status = ServiceStatus::Frozen { main_pid };
            }
        }

        Ok(())
    }

    pub fn thaw_service(&mut self, name: &str) -> Result<(), ServiceError> {
        let tx_event = self.tx_event.clone();
        let service = self.get_service_mut(name)?;

        match service.state.status {
            ServiceStatus::Stopped { .. }
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_) => {
                // No process to thaw.
            }
            ServiceStatus::Running { .. } => {
                // This process is already running.
            }
            ServiceStatus::Frozen { main_pid } => {
                kill_process_group(main_pid as pid_t, SIGCONT).expect("process to exist");
                service.state.status = ServiceStatus::Running { main_pid };
                // Resume probing now that the process is running again.
                service.spawn_readiness_probe(name.to_owned(), tx_event)
            }
        }

        Ok(())
    }

    pub fn terminate_service(
        &mut self,
        name: &str,
        reason: StopReason,
    ) -> Result<(), ServiceError> {
        let service = self.get_service_mut(name)?;

        match service.state.status {
            ServiceStatus::Stopped { .. } | ServiceStatus::Stopping { .. } => {
                // all good
            }
            ServiceStatus::Running { main_pid } | ServiceStatus::Frozen { main_pid } => {
                service.abort_readiness_probe();
                service.state.status = ServiceStatus::Stopping { main_pid, reason };
                let v = kill_process_group(main_pid as pid_t, SIGTERM).expect("process to exist");
                dbg!(v);
            }
            ServiceStatus::Exited(_) | ServiceStatus::Error(_) => {
                // nothing to do
            }
        }

        Ok(())
    }

    pub fn kill_service(&mut self, name: &str) -> Result<(), ServiceError> {
        let service = self.get_service_mut(name)?;

        match service.state.status {
            ServiceStatus::Stopped { .. } => {
                // all good
            }
            ServiceStatus::Running { .. } | ServiceStatus::Frozen { .. } => {
                panic!("service {name} was killed without being terminated")
            }
            ServiceStatus::Stopping { main_pid, .. } => {
                kill_process_group(main_pid as pid_t, SIGKILL).expect("process to exist");
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

async fn run_readiness_probe(name: String, probe: Probe, tx_event: mpsc::Sender<Event>) {
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
            eprintln!(
                "[{name}] readiness probe failed ({consecutive_failures}/{})",
                probe.failure_threshold
            );

            if consecutive_failures >= probe.failure_threshold {
                eprintln!("[{name}] readiness probe exhausted. requesting restart");
                let _ = tx_event.send(Event::ProbeFailed { name }).await;
                return;
            }
        }

        tokio::time::sleep(probe.period).await;
    }
}

fn spawn_service(
    old_sigmask: OldSigmask,
    config: &ServiceConfig,
    log_writer: std::os::unix::prelude::OwnedFd,
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

    let (mut err_rx, mut err_tx) = io::pipe()?;
    fn expect_no_panic<T>(res: io::Result<T>, msg: &'static str) -> T {
        match res {
            Ok(x) => x,
            Err(err) => {
                eprintln!("{msg}: {err}");
                _exit(101);
            }
        }
    }
    // SAFETY: We only run async-signal-safe functions inside the child process.
    let child_pid = unsafe {
        unsafe_fork!({
            expect_no_panic(old_sigmask.restore_sigmask(), "failed to restore sigmask");

            // Create a new session and process group led by this process.
            // Uses the current PID as the PGID of the new process group.
            // Using only a new process group won't work as then bash will
            // hang if the container has a tty attached.
            //
            // SAFETY: setsid is safe to call.
            expect_no_panic(cerr(libc::setsid()), "failed to setsid");

            // Set the log pipe as stdout and stderr
            // SAFETY: dup2 is memory safe to call. This technically violates IO-safety, but nothing
            // accessed after this point depends on stdout/stderr pointing to a particular fd.
            expect_no_panic(
                cerr(libc::dup2(log_writer.as_raw_fd(), 1)),
                "failed to set stdout",
            );
            expect_no_panic(
                cerr(libc::dup2(log_writer.as_raw_fd(), 2)),
                "failed to set stderr",
            );

            libc::execvp(cmd.as_ptr(), args.as_ptr());

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
        })
    }?;
    drop(err_tx);

    let mut err = [0; size_of::<i32>()];
    match err_rx.read_exact(&mut err) {
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(child_pid),
        Ok(()) => Err(io::Error::from_raw_os_error(i32::from_ne_bytes(err))),
        Err(err) => Err(err),
    }
}
