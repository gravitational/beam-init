use std::process::ExitStatus;

use serde::{Deserialize, Serialize};

pub const SOCKET_PATH: &str = "/run/beam-init";

#[derive(Serialize, Deserialize)]
pub struct CreateService {
    pub cmd: String,
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Service {
    pub cmd: String,
    pub args: Vec<String>,
    pub status: ServiceStatus,
}

#[derive(Serialize, Deserialize)]
pub enum ServiceStatus {
    /// The service was stopped by the user or hasn't been started yet.
    Stopped,

    /// The service is currently running.
    Running {
        main_pid: u32,
    },

    /// The service is paused but can be continued.
    Frozen {
        main_pid: u32,
    },

    /// The service has been requested to terminate and is in the process of shutting down.
    Stopping {
        main_pid: u32,
    },

    /// The service failed with the given exit status.
    Exited(
        #[serde(
            serialize_with = "exit_status_serde::serialize",
            deserialize_with = "exit_status_serde::deserialize"
        )]
        ExitStatus,
    ),

    Error(String),
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            ServiceStatus::Stopped => f.write_str("stopped"),
            ServiceStatus::Running { main_pid } => {
                write!(f, "running PID={main_pid}")
            }
            ServiceStatus::Frozen { main_pid } => {
                write!(f, "frozen PID={main_pid}")
            }
            ServiceStatus::Stopping { main_pid } => {
                write!(f, "stopping PID={main_pid}")
            }
            ServiceStatus::Exited(exit_status) => {
                if exit_status.success() {
                    write!(f, "exited normally")
                } else {
                    write!(f, "failed with {exit_status}")
                }
            }
            ServiceStatus::Error(ref err) => write!(f, "failed to start with {}", err),
        }
    }
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
