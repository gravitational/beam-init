use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ffi::{OsString, c_int};
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::pin::pin;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;
use std::{io, mem};

use axum::response::{IntoResponse, Response};
use futures_core::Stream;
use libc::{SIGCHLD, SIGCONT, SIGINT, SIGKILL, SIGSTOP, SIGTERM, WNOHANG, pid_t, signalfd_siginfo};
use reqwest::StatusCode;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::api_impl::{Command, Credentials};
use crate::logs::{AsyncRingBuffer, Logs};
use crate::services::service_status_publisher::{
    ServiceStatusPublisher, ServiceStatusSubscription,
};
use crate::signal_stream::OldSigmask;
use crate::spawn::{MonitorCommandSender, Sink, spawn_service};
use crate::{DEBUG_LOGS, Event};
use beam_init::BOOTSTRAP_NAME;
use beam_init::system::pty::Pty;
use beam_init::system::{kill_process_group, waitpid};
use beam_init_api::Probe;

pub(crate) struct ServiceManager {
    old_sigmask: OldSigmask,
    services: BTreeMap<String, Service>,
    tx_event: mpsc::Sender<Event>,
    user_env_files: Vec<PathBuf>,
    shutdown: CancellationToken,
}

#[derive(Debug)]
pub(crate) struct Service {
    pub(crate) config: ServiceConfig,
    pub(crate) state: ServiceState,
}

impl Service {
    fn check_credentials(&self, credentials: Credentials) -> Result<(), ServiceError> {
        if credentials.uid != 0 && credentials.uid != self.config.credentials.uid {
            return Err(ServiceError::InvalidCredentials);
        }
        Ok(())
    }

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
    fn set_status(&mut self, status: ServiceStatus) {
        self.state
            .service_status_publisher
            .publish(beam_init_api::ServiceStatus::from(&status));
        self.state.status = status;
    }
}

/// The configuration of a service.
///
/// This only changes when explicitly modified through the API.
#[derive(Debug)]
pub(crate) struct ServiceConfig {
    pub(crate) cmd: String,
    pub(crate) args: Vec<String>,
    pub(crate) env: BTreeMap<OsString, OsString>,
    pub(crate) labels: BTreeMap<String, String>,
    pub(crate) liveness: Option<Probe>,
    pub(crate) pty: bool,
    pub(crate) credentials: Credentials,
    pub(crate) graceful_termination_period: u32,
}

impl ServiceConfig {
    fn validate(&self) -> Result<(), ServiceError> {
        for (k, v) in &self.env {
            let k = k.as_os_str().as_bytes();
            let v = v.as_os_str().as_bytes();
            if k.is_empty() {
                return Err(ServiceError::InvalidRequest {
                    err: "environment keys may not be empty".to_string(),
                });
            }
            if k.contains(&b'=') {
                return Err(ServiceError::InvalidRequest {
                    err: "environment keys may not contain '='".to_string(),
                });
            }
            if k.contains(&b'\0') || v.contains(&b'\0') {
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
pub(crate) struct ServiceState {
    pub(crate) status: ServiceStatus,
    pub(crate) logs: Logs,
    pub(crate) automatic_restart_attempts: u32,
    pub(crate) liveness_probe: Option<AbortHandle>,
    service_status_publisher: ServiceStatusPublisher,
}

#[derive(Debug)]
pub(crate) enum ServiceStatus {
    /// The service was stopped by the user or hasn't been started yet.
    Stopped,

    /// The service is currently running.
    Running {
        process: ServiceProcess,
        pty: Option<Pty>,
    },

    /// The service is frozen (using SIGSTOP) but can be thawed (SIGCONT).
    Frozen {
        process: ServiceProcess,
        pty: Option<Pty>,
    },

    /// The service has been requested to terminate and is in the process of shutting down.
    /// The service will restart if the field after_stop is set to restart.
    Stopping {
        process: ServiceProcess,
        after_stop: AfterStop,
        pending_kill: Option<PendingKill>,
    },

    /// The service exited with the given exit status.
    Exited(ExitStatus),

    /// The service failed to start with the given error.
    Error(io::Error),

    /// Temporarily used inside commands that need to change the status
    Dummy,
}

#[derive(Debug)]
pub(crate) struct ServiceProcess {
    pub(crate) main_pid: pid_t,
    monitor_tx: Option<MonitorCommandSender>,
}

impl ServiceProcess {
    fn send_signal(&mut self, signal: c_int) {
        if let Some(monitor_tx) = &mut self.monitor_tx {
            monitor_tx
                .signal(signal)
                .expect("failed to send command on monitor pipe");
        } else {
            kill_process_group(self.main_pid, signal).expect("process to exist");
        }
    }
}

#[derive(Debug)]
pub enum ServiceError {
    NoPty { name: String },
    ServiceNotFound { name: String },
    ServiceExists { name: String },
    SpawnFailed { cmd: String, err: String },
    InvalidCredentials,
    InvalidRequest { err: String },
    BuildEnvironment { err: String },
    BootstrapIsProtected,
    ServicePrunedWhileWaiting,
    ObservationLagged { skipped: u64 },
    ShuttingDown,
}

// FIXME serialize as json and deserialize and format error message inside the beamctl process?
impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        match self {
            ServiceError::NoPty { name } => {
                (StatusCode::NOT_FOUND, format!("Service {name} has no PTY")).into_response()
            }
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
            ServiceError::BuildEnvironment { err } => {
                (StatusCode::INTERNAL_SERVER_ERROR, err).into_response()
            }
            ServiceError::BootstrapIsProtected => (
                StatusCode::FORBIDDEN,
                "The \"bootstrap\" service cannot be modified",
            )
                .into_response(),
            ServiceError::ServicePrunedWhileWaiting => (
                StatusCode::GONE,
                "Service was pruned before condition was met",
            )
                .into_response(),
            ServiceError::ObservationLagged{ skipped }=> (
                StatusCode::CONFLICT,
                format!("Unable to determine if requested state occurred: missed {skipped} service status updates"),
            )
                .into_response(),
            ServiceError::ShuttingDown => (
                StatusCode::SERVICE_UNAVAILABLE,
                "beam-init is shutting down",
            )
                .into_response(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum StartReason {
    /// The user requested that this service be (re)started.
    User,
    /// Beam-init requested that this service be (re)started (e.g. because it became unresponsive).
    Automatic,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AfterStop {
    Stop,
    Prune,
    Restart(StartReason),
}

impl AfterStop {
    fn merge(self, requested: Self) -> Self {
        match (self, requested) {
            // Once pruning has been requested, preserve it.
            (AfterStop::Prune, _) | (_, AfterStop::Prune) => AfterStop::Prune,

            // An automatic restart must not override an existing user action.
            (current, AfterStop::Restart(StartReason::Automatic)) => current,

            // Explicit stop, shutdown, and user restart replace the current non-prune outcome.
            (_, requested) => requested,
        }
    }
}

impl ServiceManager {
    pub fn new(
        old_sigmask: OldSigmask,
        tx_event: mpsc::Sender<Event>,
        user_env_files: Vec<PathBuf>,
        shutdown: CancellationToken,
    ) -> Self {
        ServiceManager {
            old_sigmask,
            services: BTreeMap::new(),
            tx_event,
            user_env_files,
            shutdown,
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
                    Ok(Some((pid, status))) => (pid, status),
                    Ok(None) => break,
                    Err(err) if err.raw_os_error() == Some(libc::ECHILD) => {
                        // No more zombies to wait for. While the man page of wait/waitpid only
                        // explicitly says ECHILD happens for wait when there is no child to wait
                        // for, wait is implemented in terms of waitpid, so waitpid has to have the
                        // same behavior.
                        break;
                    }
                    Err(err) => panic!("waitpid failed with {err:?}"),
                };

                for (name, service) in self.services.iter_mut() {
                    match service.state.status {
                        ServiceStatus::Running { ref process, .. } if process.main_pid == pid => {
                            service.set_status(ServiceStatus::Exited(status));
                            service.abort_liveness_probe();
                            break;
                        }
                        ServiceStatus::Stopping {
                            ref process,
                            after_stop,
                            ref mut pending_kill,
                        } if process.main_pid == pid => {
                            if let Some(pending_kill) = pending_kill.take() {
                                pending_kill.abort_handle.abort();
                            }
                            service.abort_liveness_probe();
                            match after_stop {
                                AfterStop::Stop => service.set_status(ServiceStatus::Stopped),
                                AfterStop::Prune => {
                                    let name = name.clone();
                                    self.services.remove(&name).expect("service should exist");
                                }
                                AfterStop::Restart(start_reason) => {
                                    let name = name.clone();
                                    let credentials = service.config.credentials;
                                    // start_service will set the service status to Error when an error occurs.
                                    // There is nothing else we can do with an error here, so ignore it.
                                    let _ = self.start_service(credentials, &name, start_reason);
                                }
                            }
                            break;
                        }
                        _ => { /* ignore */ }
                    }
                }
            }
        }
        if info.ssi_signo == SIGTERM as u32 || info.ssi_signo == SIGINT as u32 {
            // TODO: second signal initiates SIGKILL on all services
            self.begin_shutdown();
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
        if self.shutdown_initiated() {
            return Err(ServiceError::ShuttingDown);
        }
        if name.is_empty() {
            // Not reachable through the REST API
            return Err(ServiceError::InvalidRequest {
                err: "Service name may not be empty".to_owned(),
            });
        } else if name.len() > usize::from(u8::MAX) {
            return Err(ServiceError::InvalidRequest {
                err: "Service name may not be longer than 255 bytes".to_owned(),
            });
        }

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
                        service_status_publisher: ServiceStatusPublisher::new(
                            beam_init_api::ServiceStatus::Stopped,
                        ),
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

        service.check_credentials(credentials)?;

        Ok(service)
    }

    fn get_service_mut(
        &mut self,
        credentials: Credentials,
        name: &str,
    ) -> Result<&mut Service, ServiceError> {
        if name == BOOTSTRAP_NAME {
            return Err(ServiceError::BootstrapIsProtected);
        }

        let service = self
            .services
            .get_mut(name)
            .ok_or_else(|| ServiceError::ServiceNotFound {
                name: name.to_owned(),
            })?;

        service.check_credentials(credentials)?;

        Ok(service)
    }

    pub fn start_service(
        &mut self,
        credentials: Credentials,
        name: &str,
        reason: StartReason,
    ) -> Result<(), ServiceError> {
        if self.shutdown_initiated() {
            return Err(ServiceError::ShuttingDown);
        }
        let old_sigmask = self.old_sigmask;

        let tx_event = self.tx_event.clone();
        let service = self
            .services
            .get_mut(name)
            .ok_or_else(|| ServiceError::ServiceNotFound {
                name: name.to_owned(),
            })?;
        service.check_credentials(credentials)?;

        eprintln!("Starting service {name}");

        service.state.automatic_restart_attempts = match reason {
            StartReason::User => 0,
            StartReason::Automatic => service.state.automatic_restart_attempts.saturating_add(1),
        };

        let mut pty = service
            .config
            .pty
            .then(Pty::new)
            .transpose()
            .map_err(|err| {
                let err_str = err.to_string();
                println!("[{name}] Failed to create a pty: {err_str}");
                service.set_status(ServiceStatus::Error(err));
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
            Ok((child_pid, monitor_tx)) => {
                service.set_status(ServiceStatus::Running {
                    process: ServiceProcess {
                        main_pid: child_pid,
                        monitor_tx,
                    },
                    pty,
                });
                service.spawn_liveness_probe(name.to_owned(), tx_event);
                Ok(())
            }
            Err(err) => {
                let err_str = err.to_string();
                if *DEBUG_LOGS {
                    eprintln!("[{name}] Failed to spawn: {err_str}");
                }
                service.set_status(ServiceStatus::Error(err));
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

        match mem::replace(&mut service.state.status, ServiceStatus::Dummy) {
            status @ (ServiceStatus::Stopped
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_)) => {
                // No process to freeze.

                service.set_status(status);
            }
            status @ ServiceStatus::Frozen { .. } => {
                // This process is already frozen.
                service.set_status(status);
            }
            ServiceStatus::Running { mut process, pty } => {
                service.abort_liveness_probe();
                process.send_signal(SIGSTOP);
                service.set_status(ServiceStatus::Frozen { process, pty });
            }
            ServiceStatus::Dummy => unreachable!(),
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

        match mem::replace(&mut service.state.status, ServiceStatus::Dummy) {
            status @ (ServiceStatus::Stopped
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_)) => {
                // No process to thaw.
                service.set_status(status);
            }
            status @ ServiceStatus::Running { .. } => {
                // This process is already running.
                service.set_status(status);
            }
            ServiceStatus::Frozen { mut process, pty } => {
                process.send_signal(SIGCONT);
                service.set_status(ServiceStatus::Running { process, pty });
                // Resume probing now that the process is running again.
                service.spawn_liveness_probe(name.to_owned(), tx_event)
            }
            ServiceStatus::Dummy => unreachable!(),
        }

        Ok(())
    }

    pub fn terminate_service(
        &mut self,
        credentials: Credentials,
        name: &str,
        intent: AfterStop,
    ) -> Result<(), ServiceError> {
        let _ = self.get_service_mut(credentials, name)?;

        if self.shutdown_initiated() && matches!(intent, AfterStop::Restart(_)) {
            return Err(ServiceError::ShuttingDown);
        }

        self.terminate_service_inner(credentials, name, intent)
    }

    fn terminate_service_inner(
        &mut self,
        credentials: Credentials,
        name: &str,
        requested_after_stop: AfterStop,
    ) -> Result<(), ServiceError> {
        let service = self
            .services
            .get_mut(name)
            .ok_or_else(|| ServiceError::ServiceNotFound {
                name: name.to_owned(),
            })?;

        match mem::replace(&mut service.state.status, ServiceStatus::Dummy) {
            status @ (ServiceStatus::Stopped
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_)) => match &requested_after_stop {
                AfterStop::Stop => {
                    service.set_status(status);
                    return Ok(());
                }
                AfterStop::Prune => {
                    self.services.remove(name).expect("service should exist");
                    return Ok(());
                }
                AfterStop::Restart(start_reason) => {
                    return self.start_service(credentials, name, *start_reason);
                }
            },
            ServiceStatus::Stopping {
                process,
                after_stop,
                pending_kill,
            } => {
                service.set_status(ServiceStatus::Stopping {
                    process,
                    pending_kill,
                    after_stop: after_stop.merge(requested_after_stop),
                });
            }
            ServiceStatus::Frozen { mut process, .. } => {
                process.send_signal(SIGKILL);
                service.set_status(ServiceStatus::Stopping {
                    process,
                    after_stop: requested_after_stop,
                    pending_kill: None,
                });
            }
            ServiceStatus::Running { mut process, .. } => {
                service.abort_liveness_probe();
                process.send_signal(SIGTERM);
                let period = service.config.graceful_termination_period;
                let pending_kill = if period == 0 {
                    process.send_signal(SIGKILL);
                    None
                } else {
                    let name = name.to_string();
                    let cmd_handler = self.tx_event.clone();
                    let token = TerminationToken::default();
                    let spawn_token = token.clone();
                    let h = tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(u64::from(period))).await;
                        let (tx, _) = tokio::sync::oneshot::channel();
                        if cmd_handler
                            .send(Event::Command {
                                command: Command::ForceKillService {
                                    name: name.clone(),
                                    token: spawn_token,
                                },
                                tx,
                                credentials,
                            })
                            .await
                            .is_err()
                        {
                            eprintln!("failed to send escalate term command for service {name}");
                        };
                    });
                    let abort_handle = h.abort_handle();
                    Some(PendingKill {
                        token,
                        abort_handle,
                    })
                };
                service.set_status(ServiceStatus::Stopping {
                    process,
                    after_stop: requested_after_stop,
                    pending_kill,
                });
            }
            ServiceStatus::Dummy => unreachable!(),
        }

        Ok(())
    }

    pub fn force_kill_service(&mut self, name: &str, token: TerminationToken) {
        let Some(service) = self.services.get_mut(name) else {
            return;
        };

        let ServiceStatus::Stopping {
            process,
            after_stop: _,
            pending_kill,
        } = &mut service.state.status
        else {
            eprintln!("service {name} was not in expected Stopping state; cancelling pending kill");
            return;
        };

        if let Some(kill) = pending_kill
            && kill.token == token
        {
            process.send_signal(SIGKILL);
        }
    }

    pub fn list_services(
        &self,
        credentials: Credentials,
    ) -> impl Iterator<Item = (&String, &ServiceStatus)> {
        self.services
            .iter()
            .filter(move |(_name, service)| service.check_credentials(credentials).is_ok())
            .map(|(name, service)| (name, &service.state.status))
    }

    pub fn get_pty(&self, credentials: Credentials, name: String) -> Result<OwnedFd, ServiceError> {
        let service = self.get_service(credentials, &name)?;
        match &service.state.status {
            ServiceStatus::Running { pty, .. } | ServiceStatus::Frozen { pty, .. } => {
                if let Some(pty) = pty {
                    Ok(pty.master.try_clone().expect("dup shouldn't fail"))
                } else {
                    Err(ServiceError::NoPty { name })
                }
            }

            ServiceStatus::Stopped
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_) => Err(ServiceError::NoPty { name }),

            ServiceStatus::Dummy => unreachable!(),
        }
    }

    pub fn notify_pty_attached(
        &mut self,
        credentials: Credentials,
        name: &str,
    ) -> Result<(), ServiceError> {
        let service = self.get_service_mut(credentials, name)?;

        match &mut service.state.status {
            ServiceStatus::Running { process, pty: _ }
            | ServiceStatus::Frozen { process, pty: _ } => {
                if let Some(monitor_tx) = &mut process.monitor_tx {
                    monitor_tx
                        .foreground()
                        .expect("failed to send command on monitor pipe");
                }
            }

            ServiceStatus::Stopped
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_) => {
                // Nothing to do
            }

            ServiceStatus::Dummy => unreachable!(),
        }

        Ok(())
    }

    pub fn notify_pty_detached(
        &mut self,
        credentials: Credentials,
        name: &str,
    ) -> Result<(), ServiceError> {
        let service = self.get_service_mut(credentials, name)?;

        match &mut service.state.status {
            ServiceStatus::Running { process, pty: _ }
            | ServiceStatus::Frozen { process, pty: _ } => {
                if let Some(monitor_tx) = &mut process.monitor_tx {
                    monitor_tx
                        .background()
                        .expect("failed to send command on monitor pipe");
                }
            }

            ServiceStatus::Stopped
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_) => {
                // Nothing to do
            }

            ServiceStatus::Dummy => unreachable!(),
        }

        Ok(())
    }

    pub fn send_signal(
        &mut self,
        credentials: Credentials,
        name: &str,
        sig: c_int,
    ) -> Result<(), ServiceError> {
        let service = self.get_service_mut(credentials, name)?;

        // Check that the signal is valid to avoid an expect on EINVAL
        // Technically 32..=64 is also valid, but those are real-time signals,
        // which are unlikely to be necessary to trigger for users.
        if sig <= 0 || sig >= 32 {
            return Err(ServiceError::InvalidRequest {
                err: format!("Signal {sig} is not valid"),
            });
        }

        match &mut service.state.status {
            ServiceStatus::Running { process, pty: _ }
            | ServiceStatus::Frozen { process, pty: _ }
            | ServiceStatus::Stopping {
                process,
                after_stop: _,
                pending_kill: _,
            } => {
                process.send_signal(sig);
            }

            ServiceStatus::Stopped | ServiceStatus::Exited(_) | ServiceStatus::Error(_) => {
                // Nothing to do
            }

            ServiceStatus::Dummy => unreachable!(),
        }

        Ok(())
    }

    pub fn user_env_files(&self) -> &[PathBuf] {
        &self.user_env_files
    }

    pub fn subscribe(
        &self,
        credentials: Credentials,
        name: &str,
    ) -> Result<ServiceStatusSubscription, ServiceError> {
        let service = self.get_service(credentials, name)?;
        Ok(service.state.service_status_publisher.subscribe())
    }

    fn begin_shutdown(&mut self) {
        if self.shutdown_initiated() {
            return;
        }
        eprintln!("Starting shutdown");
        self.shutdown.cancel();
        self.stop_all_services();
    }

    pub fn shutdown_initiated(&self) -> bool {
        self.shutdown.is_cancelled()
    }

    pub fn shutdown_complete(&self) -> bool {
        self.shutdown_initiated()
            && self.services.values().all(|service| {
                matches!(
                    service.state.status,
                    ServiceStatus::Stopped | ServiceStatus::Exited(_) | ServiceStatus::Error(_)
                )
            })
    }

    fn stop_all_services(&mut self) {
        let service_names: Vec<String> = self.services.keys().cloned().collect();
        for name in service_names {
            if let Err(e) =
                self.terminate_service_inner(Credentials::root(), &name, AfterStop::Stop)
            {
                eprint!("failed to stop service {name}: {e:?}");
            }
        }
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

#[derive(Debug, Clone, Default)]
pub(crate) struct TerminationToken(Arc<()>);

impl PartialEq for TerminationToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Debug)]
pub(crate) struct PendingKill {
    token: TerminationToken,
    abort_handle: AbortHandle,
}

pub(crate) mod service_status_publisher {
    use beam_init_api::ServiceStatus;
    use tokio::sync::broadcast;

    /// Controls publishing of [ServiceStatus] updates to subscribers.
    #[derive(Debug)]
    pub(crate) struct ServiceStatusPublisher {
        latest: ServiceStatus,
        tx: broadcast::Sender<ServiceStatus>,
    }

    impl ServiceStatusPublisher {
        pub(crate) fn new(initial: ServiceStatus) -> Self {
            // Arbitrary size but should be large enough to avoid lagged readers
            let (tx, _) = broadcast::channel(32);

            Self {
                latest: initial,
                tx,
            }
        }

        pub(crate) fn publish(&mut self, status: ServiceStatus) {
            if self.latest == status {
                return;
            }
            self.latest = status.clone();

            // Errors expected when no receivers
            let _ = self.tx.send(status);
        }

        pub(crate) fn subscribe(&self) -> ServiceStatusSubscription {
            ServiceStatusSubscription {
                current: Some(self.latest.clone()),
                rx: self.tx.subscribe(),
            }
        }
    }

    /// Acts a receiver for [ServiceStatusPublisher] for service status updates.
    #[derive(Debug)]
    pub(crate) struct ServiceStatusSubscription {
        current: Option<ServiceStatus>,
        rx: broadcast::Receiver<ServiceStatus>,
    }

    impl ServiceStatusSubscription {
        pub(crate) async fn recv(&mut self) -> Result<ServiceStatus, broadcast::error::RecvError> {
            if let Some(status) = self.current.take() {
                return Ok(status);
            }
            self.rx.recv().await
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn duplicate_statuses_not_observed() {
            let mut publisher = ServiceStatusPublisher::new(ServiceStatus::Stopped);
            let mut subscription = publisher.subscribe();

            assert_eq!(
                subscription
                    .recv()
                    .await
                    .expect("first event should be initial state"),
                ServiceStatus::Stopped
            );

            publisher.publish(ServiceStatus::Running {
                main_pid: 100,
                pty: None,
            });
            publisher.publish(ServiceStatus::Running {
                main_pid: 100,
                pty: None,
            });
            publisher.publish(ServiceStatus::Exited(beam_init_api::ExitStatus::Code {
                code: 0,
            }));

            assert_eq!(
                subscription
                    .recv()
                    .await
                    .expect("subscription should receive next event"),
                ServiceStatus::Running {
                    main_pid: 100,
                    pty: None
                }
            );

            // Shouldn't see duplicate Running status
            assert_eq!(
                subscription
                    .recv()
                    .await
                    .expect("subscription should receive next event"),
                ServiceStatus::Exited(beam_init_api::ExitStatus::Code { code: 0 })
            );
        }
    }
}
