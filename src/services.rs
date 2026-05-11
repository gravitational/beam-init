use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::pin::pin;
use std::process::{Command, ExitStatus};

use futures_core::Stream;
use libc::{SIGCHLD, WNOHANG, pid_t, signalfd_siginfo};
use tokio_stream::StreamExt;

use crate::logs::Logs;
use crate::signal_stream::OldSigmask;
use crate::system::{
    continue_process_group, kill_process, stop_process_group, terminate_process, waitpid,
};

pub struct ServiceManager {
    old_sigmask: OldSigmask,
    services: BTreeMap<String, Service>,
}

pub struct Service {
    pub config: ServiceConfig,
    pub state: ServiceState,
}

/// The configuration of a service.
///
/// This only changes when explicitly modified through the API.
pub struct ServiceConfig {
    pub cmd: String,
    pub args: Vec<String>,
}

/// The runtime state of a service.
pub struct ServiceState {
    pub status: ServiceStatus,
    pub logs: Logs,
}

#[derive(Clone, Copy)]
pub enum ServiceStatus {
    /// The service was stopped by the user or hasn't been started yet.
    Stopped,

    /// The service is currently running.
    Running { main_pid: u32 },

    /// The service is frozen (using SIGSTOP) but can be thawed (SIGCONT).
    Frozen { main_pid: u32 },

    /// The service has been requested to terminate and is in the process of shutting down.
    Stopping { main_pid: u32 },

    /// The service failed with the given exit status.
    Failed(ExitStatus),
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
                        service.state.status = ServiceStatus::Failed(status);
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

    pub async fn copy_logs(&self, name: &str) -> Vec<String> {
        // FIXME error handling
        let service = self.services.get(name).unwrap();

        service.state.logs.copy_logs().await
    }

    pub fn log_reader(&self, name: &str) -> impl Stream<Item = String> + 'static {
        // FIXME error handling
        let service = self.services.get(name).unwrap();

        service.state.logs.new_reader()
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

    pub fn get_service(&self, name: &str) -> Option<&Service> {
        self.services.get(name)
    }

    pub fn start_service(&mut self, name: &str) {
        // FIXME error handling
        let service = self.services.get_mut(name).unwrap();

        println!("Starting service {name}");

        let log_writer = service.state.logs.new_writer().unwrap();
        let mut cmd = Command::new(&service.config.cmd);
        cmd.args(&service.config.args);
        cmd.stdout(log_writer.try_clone().unwrap())
            .stderr(log_writer);
        self.old_sigmask.with_restored_sigmask(&mut cmd);

        // We respond to SIGCHLD to reap zombie processes
        #[expect(clippy::zombie_processes)]
        let child = cmd.spawn().unwrap();

        service.state.status = ServiceStatus::Running {
            main_pid: child.id(),
        };
    }

    pub fn freeze_service(&mut self, name: &str) {
        // FIXME error handling
        let service = self.services.get_mut(name).unwrap();

        match service.state.status {
            ServiceStatus::Stopped | ServiceStatus::Stopping { .. } => {
                // ??
            }
            ServiceStatus::Frozen { .. } => {
                // already frozen
            }
            ServiceStatus::Running { main_pid } => {
                stop_process_group(main_pid as pid_t).unwrap();
                service.state.status = ServiceStatus::Frozen { main_pid };
            }
            ServiceStatus::Failed(_) => {
                // ??
            }
        }
    }

    pub fn thaw_service(&mut self, name: &str) {
        // FIXME error handling
        let service = self.services.get_mut(name).unwrap();

        match service.state.status {
            ServiceStatus::Stopped | ServiceStatus::Stopping { .. } => {
                // ??
            }
            ServiceStatus::Running { .. } => {
                // already running
            }
            ServiceStatus::Frozen { main_pid } => {
                continue_process_group(main_pid as pid_t).unwrap();
                service.state.status = ServiceStatus::Running { main_pid };
            }
            ServiceStatus::Failed(_) => {
                // ??
            }
        }
    }

    pub fn terminate_service(&mut self, name: &str) {
        // FIXME error handling
        let service = self.services.get_mut(name).unwrap();

        match service.state.status {
            ServiceStatus::Stopped | ServiceStatus::Stopping { .. } => {
                // all good
            }
            ServiceStatus::Running { main_pid } | ServiceStatus::Frozen { main_pid } => {
                service.state.status = ServiceStatus::Stopping { main_pid };
                terminate_process(main_pid as pid_t).unwrap();
            }
            ServiceStatus::Failed(_) => {
                // nothing to do
            }
        }
    }

    pub fn kill_service(&mut self, name: &str) {
        // FIXME error handling
        let service = self.services.get_mut(name).unwrap();

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
            ServiceStatus::Failed(_) => {
                // nothing to do
            }
        }
    }

    pub fn list_services(&self) -> impl Iterator<Item = (&String, ServiceStatus)> {
        self.services
            .iter()
            .map(|(name, service)| (name, service.state.status))
    }
}
