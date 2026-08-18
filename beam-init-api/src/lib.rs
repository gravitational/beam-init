//! Shared request and response types for the beam-init Unix-socket API.
//!
//! The types in this crate define the JSON exchanged between beam-init and its
//! clients.

use std::{collections::BTreeMap, path::PathBuf, process::ExitStatus, time::Duration};

use libc::pid_t;
use serde::{Deserialize, Serialize};

/// Default Unix socket path for the beam-init HTTP API.
pub const API_SOCKET_PATH: &str = "/run/beam-init";

/// Default Unix socket path used to retrieve file descriptors from beam-init.
pub const FD_SOCKET_PATH: &str = "/run/beam-init-fds";

/// Request body for creating and starting a service.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateService {
    /// Executable to run.
    pub cmd: String,

    /// Arguments passed to the executable.
    pub args: Vec<String>,

    /// Optional HTTP liveness probe configuration.
    pub liveness: Option<Probe>,

    /// Whether to run the service with a controlling pseudoterminal.
    pub pty: bool,

    /// Labels attached to a service
    pub labels: BTreeMap<String, String>,
}

/// Configuration for an HTTP liveness probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Probe {
    /// HTTP request path used by the probe.
    pub path: String,

    /// Local TCP port on which the service exposes its probe endpoint.
    pub port: u16,

    /// Maximum number of retries after failed probes before probing stops.
    pub max_retries: usize,

    /// Delay between starting the service and sending the first probe.
    pub initial_delay: Duration,

    /// Timeout for each probe request and delay between attempts.
    pub period: Duration,

    /// Consecutive failures required to restart the service.
    pub failure_threshold: usize,
}

/// Description and current state of a service.
#[derive(Serialize, Deserialize)]
pub struct Service {
    /// Executable configured for the service.
    pub cmd: String,

    /// Arguments passed to the executable.
    pub args: Vec<String>,

    /// Current runtime status of the service.
    pub status: ServiceStatus,

    /// Number of automatic restart attempts since start.
    pub automatic_restart_attempts: u32,
}

/// Current runtime state of a service.
#[derive(Serialize, Deserialize)]
pub enum ServiceStatus {
    /// The service was stopped by the user or hasn't been started yet.
    Stopped,

    /// The service is currently running.
    Running {
        /// Process ID of the service's main process.
        main_pid: pid_t,

        /// Labels of the service
        labels: BTreeMap<String, String>,

        /// File-descriptor store ID and device path for the service's PTY, if allocated.
        pty: Option<(u64, PathBuf)>,
    },

    /// The service is paused but can be continued.
    Frozen {
        /// Process ID of the service's main process.
        main_pid: pid_t,

        /// Labels of the service
        labels: BTreeMap<String, String>,

        /// File-descriptor store ID and device path for the service's PTY, if allocated.
        pty: Option<(u64, PathBuf)>,
    },

    /// The service has been requested to restart and is in the process of shutting down.
    Restarting {
        /// Process ID of the service instance being stopped.
        main_pid: pid_t,

        /// Name under which the replacement service will be started.
        name: String,
    },

    /// The service has been requested to terminate and is in the process of shutting down.
    Stopping {
        /// Process ID of the service's main process.
        main_pid: pid_t,

        /// Whether to remove the service after it stops.
        prune: bool,
    },

    /// The service exited with the given exit status.
    Exited(
        #[serde(
            serialize_with = "exit_status_serde::serialize",
            deserialize_with = "exit_status_serde::deserialize"
        )]
        ExitStatus,
    ),

    /// The service failed to start with the given error.
    Error(String),
}

/// Version information for beam-init
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionResponse {
    /// The version reported by beam-init
    pub version: String,
    /// The commit sha beam-init was compiled from
    pub sha: String,
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceStatus::Stopped => f.write_str("stopped"),
            ServiceStatus::Running {
                main_pid,
                labels,
                pty,
            } => {
                write!(f, "running PID={main_pid}")?;
                if let Some((_, path)) = pty {
                    write!(f, ", pty={}", path.display())?;
                }
                write_labels(labels, f)?;
                Ok(())
            }
            ServiceStatus::Frozen {
                main_pid,
                labels,
                pty,
            } => {
                write!(f, "frozen PID={main_pid}")?;
                if let Some((_, path)) = pty {
                    write!(f, ", pty={}", path.display())?;
                }
                write_labels(labels, f)?;
                Ok(())
            }
            ServiceStatus::Stopping { main_pid, prune } => {
                write!(f, "stopping PID={main_pid} (prune={prune})")
            }
            ServiceStatus::Restarting { main_pid, name: _ } => {
                write!(f, "restarting PID={main_pid}")
            }
            ServiceStatus::Exited(exit_status) => {
                if exit_status.success() {
                    write!(f, "exited normally")
                } else {
                    write!(f, "failed with {exit_status}")
                }
            }
            ServiceStatus::Error(err) => write!(f, "failed to start with {}", err),
        }
    }
}

// https://doc.rust-lang.org/std/iter/struct.Intersperse.html is not stable yet, but we can lego it
fn intersperse<Sep, T>(sep: Sep, mut iter: impl Iterator<Item = T>) -> impl Iterator<Item = T>
where
    Sep: Copy,
    T: From<Sep>,
{
    let mut do_sep = false;
    std::iter::from_fn(move || {
        if do_sep {
            do_sep = false;
            Some(sep.into())
        } else {
            do_sep = true;
            iter.next()
        }
    })
}

fn write_labels(
    labels: &BTreeMap<String, String>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    if labels.is_empty() {
        return Ok(());
    }

    write!(f, ", labels=[")?;
    for representation in intersperse(
        ",",
        labels.iter().map(|(key, value)| format!("{key}={value}")),
    ) {
        write!(f, "{}", representation)?;
    }
    write!(f, "]")?;

    Ok(())
}

/// Functions to serialize and deserialize ExitStatus
mod exit_status_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    pub fn serialize<S>(status: &ExitStatus, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i32(status.into_raw())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ExitStatus, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = i32::deserialize(deserializer)?;
        Ok(ExitStatus::from_raw(raw))
    }
}
