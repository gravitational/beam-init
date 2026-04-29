use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::process::{Command, ExitStatus};

use libc::{SIGCHLD, WNOHANG, pid_t, signalfd_siginfo};

use crate::signal_stream::OldSigmask;
use crate::system::{kill_process, terminate_process, waitpid};

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
}

pub enum ServiceStatus {
    /// The service was stopped by the user or hasn't been started yet.
    Stopped,

    /// The service is currently running.
    Running { main_pid: u32 },

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

    pub fn create_service(&mut self, name: String, config: ServiceConfig) {
        match self.services.entry(name) {
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(Service {
                    config,
                    state: ServiceState {
                        status: ServiceStatus::Stopped,
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
        let mut cmd = Command::new(&service.config.cmd);
        cmd.args(&service.config.args);
        self.old_sigmask.with_restored_sigmask(&mut cmd);

        // We respond to SIGCHLD to reap zombie processes
        #[expect(clippy::zombie_processes)]
        let child = cmd.spawn().unwrap();

        service.state.status = ServiceStatus::Running {
            main_pid: child.id(),
        };
    }

    pub fn terminate_service(&mut self, name: &str) {
        // FIXME error handling
        let service = self.services.get_mut(name).unwrap();

        match service.state.status {
            ServiceStatus::Stopped | ServiceStatus::Stopping { .. } => {
                // all good
            }
            ServiceStatus::Running { main_pid } => {
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
            ServiceStatus::Running { .. } => {
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
}
