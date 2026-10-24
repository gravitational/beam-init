#![deny(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::os::fd::OwnedFd;
use std::os::unix::process::ExitStatusExt;
use std::sync::LazyLock;
use std::{env, process};

use axum::response::{IntoResponse, Response};
use clap::Parser;
use libc::{SIGCHLD, SIGINT, SIGTERM, signalfd_siginfo};
use tokio::sync::oneshot;
use tokio::sync::watch::Receiver;

use crate::api_impl::Credentials;
use crate::services::{ServiceManager, ServiceObservation, ServiceStatus, StartReason, StopIntent};
use beam_init::BOOTSTRAP_NAME;
use beam_init::system::exit_with_signal;

mod api_impl;
mod cli;
mod environment;
mod logs;
mod ptystore;
mod services;
mod signal_stream;
mod spawn;

pub(crate) const VERSION: &str = match option_env!("BEAM_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
pub(crate) const GIT_SHA: &str = match option_env!("GIT_SHA") {
    Some(sha) => sha,
    None => "unknown",
};
/// If true we will print log messages that may contain sensitive information.
///
/// Set to true using the `BEAM_INIT_ENABLE_DEBUG_LOGS=1` env var.
static DEBUG_LOGS: LazyLock<bool> =
    LazyLock::new(|| env::var("BEAM_INIT_ENABLE_DEBUG_LOGS").as_deref() == Ok("1"));

#[derive(Debug)]
enum Event {
    Command {
        command: api_impl::Command,
        tx: oneshot::Sender<Response>,
        credentials: Credentials,
    },
    GetPty {
        name: String,
        tx: oneshot::Sender<OwnedFd>,
        credentials: Credentials,
    },
    Signal(signalfd_siginfo),
    ProbeFailed {
        name: String,
    },
    WaitService {
        name: String,
        credentials: Credentials,
        tx: oneshot::Sender<Result<Receiver<ServiceObservation>, services::ServiceError>>,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    eprintln!("Starting beam-init");

    // Mark ourself as child subreaper to allow testing outside of a container. This ensures we can
    // act as reaper for all our (indirect) child processes without needing to be pid 1.
    // Safety: This prctl can't violate memory safety.
    unsafe {
        libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1);
    }

    // FIXME what is a reasonable channel capacity?
    let (tx_event, mut rx_event) = tokio::sync::mpsc::channel(10);

    let mut cli = cli::Cli::parse();
    let cmd = cli.command.remove(0);

    // Queue an API command to start the first service
    let init_cmd = api_impl::Command::CreateService {
        name: beam_init::BOOTSTRAP_NAME.to_owned(),
        service: beam_init_api::CreateService {
            cmd,
            args: cli.command,
            env: BTreeMap::new(),
            liveness: None,
            pty: false,
            labels: Default::default(),
            graceful_termination_period: cli.bootstrap_graceful_termination_period,
        },
    };
    // The channel is empty, so sending always succeeds.
    tx_event
        .try_send(Event::Command {
            command: init_cmd,
            tx: oneshot::channel().0,
            credentials: Credentials::root(),
        })
        .expect("channel should be empty");

    // Listen for SIGCHLD signals
    let old_sigmask = signal_stream::init(&[SIGCHLD, SIGTERM, SIGINT], tx_event.clone())
        .expect("failed to initialize the signal stream");

    let shutdown = tokio_util::sync::CancellationToken::new();

    ptystore::init(tx_event.clone(), shutdown.child_token())
        .expect("failed to bind ptystore socket");

    // Listen for API commands
    api_impl::bind_api_socket(tx_event.clone(), shutdown.child_token())
        .expect("failed to bind api socket");

    let mut service_manager =
        ServiceManager::new(old_sigmask, tx_event, cli.environment_file, shutdown);
    loop {
        match rx_event
            .recv()
            .await
            .expect("signal stream and api socket tasks failed")
        {
            Event::Signal(info) => service_manager.handle_signal(info),
            Event::Command {
                command: cmd,
                tx,
                credentials,
            } => {
                let res =
                    api_impl::handle_api_command(&mut service_manager, cmd, credentials).await;
                let _ = tx.send(res.into_response());
            }
            Event::GetPty {
                name,
                tx,
                credentials,
            } => match service_manager.get_pty(credentials, name) {
                Ok(res) => {
                    let _ = tx.send(res);
                }
                Err(err) => {
                    if *DEBUG_LOGS {
                        eprintln!("failed to get pty: {err:?}");
                    }
                }
            },
            Event::ProbeFailed { name } => {
                if let Err(e) = api_impl::stop_service_cmd(
                    &mut service_manager,
                    Credentials::root(),
                    &name,
                    StopIntent::Restart(StartReason::Automatic),
                )
                .await
                {
                    if *DEBUG_LOGS {
                        eprintln!("failed to automatically restart {name}: {e:?}");
                    }
                    if let Ok(service) = service_manager.get_service(Credentials::root(), &name) {
                        service
                            .state
                            .logs
                            .queue
                            .push(format!("[failed to automatically restart: {e:?}]"))
                            .await;
                    }
                }
            }
            Event::WaitService {
                name,
                credentials,
                tx,
            } => {
                let rx = service_manager.subscribe(credentials, &name);
                let _ = tx.send(rx);
            }
        }

        if service_manager.shutdown_complete() {
            break;
        }

        if service_manager.shutdown_initiated() {
            continue;
        }

        if let Ok(service) = service_manager.get_service(Credentials::root(), BOOTSTRAP_NAME)
            && let ServiceStatus::Exited(status) = service.state.status
        {
            if let Some(code) = status.code() {
                process::exit(code);
            } else if let Some(signal) = status.signal() {
                exit_with_signal(signal)
            } else {
                process::exit(1);
            }
        }
        if let Ok(service) = service_manager.get_service(Credentials::root(), BOOTSTRAP_NAME)
            && let ServiceStatus::Stopped = service.state.status
        {
            process::exit(0);
        }
    }
}
