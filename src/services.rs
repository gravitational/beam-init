use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::process::{Command, ExitStatus};

use libc::{SIGCHLD, WNOHANG, pid_t, signalfd_siginfo};

use crate::signal_stream::OldSigmask;
use crate::system::waitpid;

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
                if let ServiceStatus::Running { main_pid } = service.state.status
                    && main_pid == info.ssi_pid
                {
                    service.state.status = ServiceStatus::Failed(status);
                    return;
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
}
