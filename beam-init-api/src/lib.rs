//! Shared request and response types for the beam-init Unix-socket API.
//!
//! The types in this crate define the JSON exchanged between beam-init and its
//! clients.

use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt;
use std::{path::PathBuf, process::ExitStatus as StdExitStatus, time::Duration};

use libc::pid_t;
use serde::{Deserialize, Serialize};

/// Default Unix socket path for the beam-init HTTP API.
pub const API_SOCKET_PATH: &str = "/run/beam-init";

/// Default Unix socket path used to retrieve PTY file descriptors from beam-init.
pub const PTY_SOCKET_PATH: &str = "/run/beam-init-ptys";

/// Request body for creating and starting a service.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateService {
    /// Executable to run.
    pub cmd: String,

    /// Arguments passed to the executable.
    pub args: Vec<String>,

    /// Environment variables
    #[serde(default)]
    pub env: BTreeMap<String, String>,

    /// Optional HTTP liveness probe configuration.
    pub liveness: Option<Probe>,

    /// Whether to run the service with a controlling pseudoterminal.
    pub pty: bool,

    /// Labels attached to a service
    pub labels: BTreeMap<String, String>,

    // The amount of time, measured in seconds, a service has to shutdown after a SIGTERM before
    // it will be sent a SIGKILL. If unset, a SIGKILL is sent.
    #[serde(default)]
    pub graceful_termination_period: u32,
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

    /// Labels of the service
    pub labels: BTreeMap<String, String>,
}

/// Current runtime state of a service.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    /// The service was stopped by the user or hasn't been started yet.
    Stopped,

    /// The service is currently running.
    Running {
        /// Process ID of the service's main process.
        main_pid: pid_t,

        /// Device path for the service's PTY, if allocated.
        pty: Option<PathBuf>,
    },

    /// The service is paused but can be continued.
    Frozen {
        /// Process ID of the service's main process.
        main_pid: pid_t,

        /// Device path for the service's PTY, if allocated.
        pty: Option<PathBuf>,
    },

    /// The service has been requested to terminate and is in the process of shutting down.
    Stopping {
        /// Process ID of the service's main process.
        main_pid: pid_t,

        /// Whether to remove the service after it stops.
        prune: bool,

        /// Whether the service will restart after stopped.
        will_restart: bool,
    },

    /// The service exited with the given exit status.
    Exited(ExitStatus),

    /// The service failed to start with the given error.
    Error(String),
}

/// The result of a service process exiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ExitStatus {
    /// The process exited normally with the given exit code.
    #[serde(rename = "exited")]
    Code { code: i32 },

    /// The process was terminated by the given signal number.
    #[serde(rename = "signaled")]
    Signal { signal: i32, core_dumped: bool },
}

impl From<StdExitStatus> for ExitStatus {
    fn from(status: StdExitStatus) -> Self {
        if let Some(code) = status.code() {
            Self::Code { code }
        } else if let Some(signal) = status.signal() {
            Self::Signal {
                signal,
                core_dumped: status.core_dumped(),
            }
        } else {
            unreachable!("waitpid returned a status that was neither exited nor signaled")
        }
    }
}

/// Version information for beam-init
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionResponse {
    /// The version reported by beam-init
    pub version: String,
    /// The commit sha beam-init was compiled from
    pub sha: String,
}

/// Returns the name of a standard Linux signal.
///
/// Modified from Rust's standard library implementation:
/// https://github.com/rust-lang/rust/blob/019b59d8101bb3e141f1b880684747b4217beeb6/library/std/src/sys/process/unix/unix.rs#L1147-L1249
fn signal_name(signal: i32) -> Option<&'static str> {
    match signal {
        libc::SIGHUP => Some("SIGHUP"),
        libc::SIGINT => Some("SIGINT"),
        libc::SIGQUIT => Some("SIGQUIT"),
        libc::SIGILL => Some("SIGILL"),
        libc::SIGTRAP => Some("SIGTRAP"),
        libc::SIGABRT => Some("SIGABRT"),
        libc::SIGBUS => Some("SIGBUS"),
        libc::SIGFPE => Some("SIGFPE"),
        libc::SIGKILL => Some("SIGKILL"),
        libc::SIGUSR1 => Some("SIGUSR1"),
        libc::SIGSEGV => Some("SIGSEGV"),
        libc::SIGUSR2 => Some("SIGUSR2"),
        libc::SIGPIPE => Some("SIGPIPE"),
        libc::SIGALRM => Some("SIGALRM"),
        libc::SIGTERM => Some("SIGTERM"),
        libc::SIGCHLD => Some("SIGCHLD"),
        libc::SIGCONT => Some("SIGCONT"),
        libc::SIGSTOP => Some("SIGSTOP"),
        libc::SIGTSTP => Some("SIGTSTP"),
        libc::SIGTTIN => Some("SIGTTIN"),
        libc::SIGTTOU => Some("SIGTTOU"),
        libc::SIGURG => Some("SIGURG"),
        libc::SIGXCPU => Some("SIGXCPU"),
        libc::SIGXFSZ => Some("SIGXFSZ"),
        libc::SIGVTALRM => Some("SIGVTALRM"),
        libc::SIGPROF => Some("SIGPROF"),
        libc::SIGWINCH => Some("SIGWINCH"),
        libc::SIGIO => Some("SIGIO"),
        libc::SIGSYS => Some("SIGSYS"),
        #[cfg(any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64"
        ))]
        libc::SIGSTKFLT => Some("SIGSTKFLT"),
        libc::SIGPWR => Some("SIGPWR"),
        _ => None,
    }
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceStatus::Stopped => f.write_str("stopped"),
            ServiceStatus::Running { main_pid, pty } => {
                write!(f, "running PID={main_pid}")?;
                if let Some(path) = pty {
                    write!(f, ", pty={}", path.display())?;
                }
                Ok(())
            }
            ServiceStatus::Frozen { main_pid, pty } => {
                write!(f, "frozen PID={main_pid}")?;
                if let Some(path) = pty {
                    write!(f, ", pty={}", path.display())?;
                }
                Ok(())
            }
            ServiceStatus::Stopping {
                main_pid,
                prune,
                will_restart,
            } => {
                write!(
                    f,
                    "stopping PID={main_pid} (prune={prune}, will_restart={will_restart})"
                )
            }
            ServiceStatus::Exited(ExitStatus::Code { code: 0 }) => write!(f, "exited normally"),
            ServiceStatus::Exited(ExitStatus::Code { code }) => {
                write!(f, "failed with exit status: {code}")
            }
            ServiceStatus::Exited(ExitStatus::Signal {
                signal,
                core_dumped,
            }) => {
                write!(f, "failed with signal: {signal}")?;
                if let Some(name) = signal_name(*signal) {
                    write!(f, " ({name})")?;
                }
                if *core_dumped {
                    f.write_str(" (core dumped)")?;
                }
                Ok(())
            }
            ServiceStatus::Error(err) => write!(f, "failed to start with {}", err),
        }
    }
}
