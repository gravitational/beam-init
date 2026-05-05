use std::process::ExitStatus;

use serde::{Deserialize, Serialize};

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

impl From<crate::services::Service> for Service {
    fn from(value: crate::services::Service) -> Self {
        Self {
            cmd: value.config.cmd,
            args: value.config.args,
            status: value.state.status.into(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum ServiceStatus {
    /// The service was stopped by the user or hasn't been started yet.
    Stopped,

    /// The service is currently running.
    Running { main_pid: u32 },

    /// The service has been requested to terminate and is in the process of shutting down.
    Stopping { main_pid: u32 },

    /// The service failed with the given exit status.
    Failed(
        #[serde(
            serialize_with = "exit_status_serde::serialize",
            deserialize_with = "exit_status_serde::deserialize"
        )]
        ExitStatus,
    ),
}

impl From<crate::services::ServiceStatus> for ServiceStatus {
    fn from(value: crate::services::ServiceStatus) -> Self {
        match value {
            crate::services::ServiceStatus::Stopped => ServiceStatus::Stopped,
            crate::services::ServiceStatus::Running { main_pid } => {
                ServiceStatus::Running { main_pid }
            }
            crate::services::ServiceStatus::Stopping { main_pid } => {
                ServiceStatus::Stopping { main_pid }
            }
            crate::services::ServiceStatus::Failed(exit_status) => {
                ServiceStatus::Failed(exit_status)
            }
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
