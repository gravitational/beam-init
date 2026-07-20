#![deny(clippy::unwrap_used)]

use std::os::unix::process::ExitStatusExt;
use std::sync::LazyLock;
use std::{env, process};

use axum::response::{IntoResponse, Response};
use libc::{SIGCHLD, signalfd_siginfo};
use tokio::sync::oneshot;

use crate::api_impl::Credentials;
use crate::services::{ServiceManager, ServiceStatus};
use crate::system::exit_with_signal;
use beam_init::api;

mod api_impl;
mod fdstore;
mod logs;
mod services;
mod signal_stream;
mod system;

/// If true we will print log messages that may contain sensitive information.
///
/// Set to true using the `BEAM_INIT_ENABLE_DEBUG_LOGS=1` env var.
static DEBUG_LOGS: LazyLock<bool> =
    LazyLock::new(|| env::var("BEAM_INIT_ENABLE_DEBUG_LOGS").as_deref() == Ok("1"));

enum Event {
    Command {
        command: api_impl::Command,
        tx: oneshot::Sender<Response>,
        credentials: Credentials,
    },
    Signal(signalfd_siginfo),
    ProbeFailed {
        name: String,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    eprintln!("Starting beam-init");

    // FIXME what is a reasonable channel capacity?
    let (tx_event, mut rx_event) = tokio::sync::mpsc::channel(10);

    // Queue a fake API command to start the first service
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| {
        eprintln!("Usage: beam-init <COMMAND>...");
        process::exit(2);
    });
    let init_cmd = api_impl::Command::CreateService {
        name: "bootstrap".to_owned(),
        service: api::CreateService {
            cmd,
            args: args.collect(),
            liveness: None,
            pty: false,
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
    let old_sigmask = signal_stream::init(&[SIGCHLD], tx_event.clone())
        .expect("failed to initialize the signal stream");

    let fdstore = if cfg!(feature = "unstable-pty")
        && env::var("BEAM_INIT_ENABLE_API").as_deref() == Ok("1")
    {
        fdstore::FdStore::bind_socket().expect("failed to bind fdstore socket")
    } else {
        fdstore::FdStore::no_socket()
    };

    if env::var("BEAM_INIT_ENABLE_API").as_deref() == Ok("1") {
        // Listen for API commands
        api_impl::bind_api_socket(tx_event.clone()).expect("failed to bind api socket");
    }

    let mut service_manager = ServiceManager::new(old_sigmask, tx_event, fdstore);
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
            Event::ProbeFailed { name } => {
                if let Err(e) = api_impl::automatic_restart(&mut service_manager, &name).await {
                    if *DEBUG_LOGS {
                        eprintln!("failed to automatically restart {name}: {e:?}");
                    }
                    if let Ok(service) = service_manager.get_service(&name) {
                        service
                            .state
                            .logs
                            .queue
                            .push(format!("[failed to automatically restart: {e:?}]"))
                            .await;
                    }
                }
            }
        }

        if let Ok(service) = service_manager.get_service("bootstrap")
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
        if let Ok(service) = service_manager.get_service("bootstrap")
            && let ServiceStatus::Stopped = service.state.status
        {
            process::exit(0);
        }
    }
}
