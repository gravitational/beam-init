use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ffi::{CString, NulError};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::ExitStatusExt;
use std::pin::pin;
use std::process::{self, ExitStatus};
use std::ptr;

use axum::response::{IntoResponse, Response};
use futures_core::Stream;
use libc::{SIGCHLD, WNOHANG, execvp, pid_t, signalfd_siginfo};
use reqwest::StatusCode;
use tokio_stream::StreamExt;

use crate::logs::Logs;
use crate::signal_stream::OldSigmask;
use crate::system::fork::unsafe_fork;
use crate::system::{
    cerr, continue_process_group, kill_process, stop_process_group, terminate_process, waitpid,
};

pub struct ServiceManager {
    old_sigmask: OldSigmask,
    services: BTreeMap<String, Service>,
}

#[derive(Debug)]
pub struct Service {
    pub config: ServiceConfig,
    pub state: ServiceState,
}

/// The configuration of a service.
///
/// This only changes when explicitly modified through the API.
#[derive(Debug)]
pub struct ServiceConfig {
    pub cmd: String,
    pub args: Vec<String>,
}

/// The runtime state of a service.
#[derive(Debug)]
pub struct ServiceState {
    pub status: ServiceStatus,
    pub logs: Logs,
}

#[derive(Debug)]
pub enum ServiceStatus {
    /// The service was stopped by the user or hasn't been started yet.
    Stopped,

    /// The service is currently running.
    Running { main_pid: u32 },

    /// The service is frozen (using SIGSTOP) but can be thawed (SIGCONT).
    Frozen { main_pid: u32 },

    /// The service has been requested to terminate and is in the process of shutting down.
    Stopping { main_pid: u32 },

    /// The service exited with the given exit status.
    Exited(ExitStatus),

    /// The service failed to start with the given error.
    Error(io::Error),
}

#[derive(Debug)]
pub enum ServiceError {
    ServiceNotFound { name: String },
    SpawnFailed { cmd: String, err: io::Error },
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
            ServiceError::SpawnFailed { cmd, err } => (
                StatusCode::BAD_REQUEST,
                format!("Failed to spawn {cmd}: {err}"),
            )
                .into_response(),
        }
    }
}

impl ServiceManager {
    pub fn new(old_sigmask: OldSigmask) -> Self {
        ServiceManager {
            old_sigmask,
            services: BTreeMap::new(),
        }
    }

    pub fn handle_signal(&mut self, info: signalfd_siginfo) {
        if info.ssi_signo == SIGCHLD as u32 {
            let (pid, status) = waitpid(info.ssi_pid as pid_t, WNOHANG).unwrap();
            if pid == 0 {
                return;
            }

            for service in self.services.values_mut() {
                match service.state.status {
                    ServiceStatus::Running { main_pid } if main_pid == info.ssi_pid => {
                        service.state.status = ServiceStatus::Exited(status);
                        return;
                    }
                    ServiceStatus::Stopping { main_pid } if main_pid == info.ssi_pid => {
                        service.state.status = ServiceStatus::Stopped;
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

    pub fn create_service(&mut self, name: String, config: ServiceConfig) {
        let logs = Logs::new();

        let reader = logs.new_reader();
        let name2 = name.clone();
        tokio::spawn(async move {
            let mut reader = pin!(reader);
            while let Some(line) = reader.next().await {
                println!("[{name2}] {line}");
            }
        });

        match self.services.entry(name) {
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(Service {
                    config,
                    state: ServiceState {
                        status: ServiceStatus::Stopped,
                        logs,
                    },
                });
            }
            // FIXME error handling
            Entry::Occupied(_) => todo!("service already exists"),
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

    pub fn start_service(&mut self, name: &str) -> Result<(), ServiceError> {
        let old_sigmask = self.old_sigmask;

        let service = self.get_service_mut(name)?;

        println!("Starting service {name}");

        let log_writer = service.state.logs.new_writer().unwrap();

        match spawn_service(old_sigmask, &service.config, log_writer) {
            Ok(child_pid) => {
                service.state.status = ServiceStatus::Running {
                    main_pid: child_pid as u32,
                };
                Ok(())
            }
            Err(err) => {
                println!("[{name}] Failed to spawn: {err}");
                // FIXME do something better than ExitStatus::from_raw(101 << 8)
                service.state.status = ServiceStatus::Exited(ExitStatus::from_raw(101 << 8));
                Err(ServiceError::SpawnFailed {
                    cmd: service.config.cmd.clone(),
                    err,
                })
            }
        }
    }

    pub fn freeze_service(&mut self, name: &str) -> Result<(), ServiceError> {
        let service = self.get_service_mut(name)?;

        match service.state.status {
            ServiceStatus::Stopped
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_) => {
                // No process to freeze.
            }
            ServiceStatus::Frozen { .. } => {
                // This process is already frozen.
            }
            ServiceStatus::Running { main_pid } => {
                stop_process_group(main_pid as pid_t).unwrap();
                service.state.status = ServiceStatus::Frozen { main_pid };
            }
        }

        Ok(())
    }

    pub fn thaw_service(&mut self, name: &str) -> Result<(), ServiceError> {
        let service = self.get_service_mut(name)?;

        match service.state.status {
            ServiceStatus::Stopped
            | ServiceStatus::Stopping { .. }
            | ServiceStatus::Exited(_)
            | ServiceStatus::Error(_) => {
                // No process to thaw.
            }
            ServiceStatus::Running { .. } => {
                // This process is already running.
            }
            ServiceStatus::Frozen { main_pid } => {
                continue_process_group(main_pid as pid_t).unwrap();
                service.state.status = ServiceStatus::Running { main_pid };
            }
        }

        Ok(())
    }

    pub fn terminate_service(&mut self, name: &str) -> Result<(), ServiceError> {
        let service = self.get_service_mut(name)?;

        match service.state.status {
            ServiceStatus::Stopped | ServiceStatus::Stopping { .. } => {
                // all good
            }
            ServiceStatus::Running { main_pid } | ServiceStatus::Frozen { main_pid } => {
                service.state.status = ServiceStatus::Stopping { main_pid };
                terminate_process(main_pid as pid_t).unwrap();
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
            ServiceStatus::Stopped => {
                // all good
            }
            ServiceStatus::Running { .. } | ServiceStatus::Frozen { .. } => {
                panic!("service {name} was killed without being terminated")
            }
            ServiceStatus::Stopping { main_pid } => {
                // `handle_signal` will update the status.
                if let Err(e) = kill_process(main_pid as pid_t)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    // NotFound means that we tried to kill a process that already exited.
                    todo!()
                }
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
    // SAFETY: We only run async-signal-safe functions inside the child process.
    let child_pid = unsafe {
        unsafe_fork!({
            old_sigmask
                .restore_sigmask()
                .expect("failed to restore sigmask");

            // Create a new session and process group led by this process.
            // Uses the current PID as the PGID of the new process group.
            // Using only a new process group won't work as then bash will
            // hang if the container has a tty attached.
            //
            // SAFETY: setsid is safe to call.
            cerr(libc::setsid()).expect("failed to setsid");

            // Set the log pipe as stdout and stderr
            // SAFETY: dup2 is memory safe to call. This technically violates IO-safety, but nothing
            // accessed after this point depends on stdout/stderr pointing to a particular fd.
            cerr(libc::dup2(log_writer.as_raw_fd(), 1)).expect("failed to set stdout");
            cerr(libc::dup2(log_writer.as_raw_fd(), 2)).expect("failed to set stderr");

            execvp(cmd.as_ptr(), args.as_ptr());
            let err = io::Error::last_os_error()
                .raw_os_error()
                .expect("last_os_error didn't return OS error");

            err_tx
                .write_all(&i32::to_ne_bytes(err))
                .expect("failed to write error code");
            process::exit(1)
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
