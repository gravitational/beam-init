use std::os::unix::process::ExitStatusExt;
use std::{env, process};

use axum::response::{IntoResponse, Response};
use libc::{SIGCHLD, signalfd_siginfo};
use tokio::sync::oneshot;

use crate::services::{ServiceManager, ServiceStatus};
use crate::system::exit_with_signal;
use beam_init::api;

mod api_impl;
mod logs;
mod services;
mod signal_stream;
mod system;

enum Event {
    Command(api_impl::Command, oneshot::Sender<Response>),
    Signal(signalfd_siginfo),
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("Starting beam-init");

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
        },
    };
    // The channel is empty, so sending always succeeds.
    tx_event
        .try_send(Event::Command(init_cmd, oneshot::channel().0))
        .expect("channel should be empty");

    // Listen for SIGCHLD signals
    let old_sigmask = signal_stream::init(&[SIGCHLD], tx_event.clone())
        .expect("failed to initialize the signal stream");

    if env::var("BEAM_INIT_ENABLE_API").as_deref() == Ok("1") {
        // Listen for API commands
        api_impl::bind_api_socket(tx_event).expect("failed to bind api socket");
    }

    let mut service_manager = ServiceManager::new(old_sigmask);
    loop {
        match rx_event
            .recv()
            .await
            .expect("signal stream and api socket tasks failed")
        {
            Event::Signal(info) => service_manager.handle_signal(info),
            Event::Command(cmd, tx) => {
                let res = api_impl::handle_api_command(&mut service_manager, cmd).await;
                let _ = tx.send(res.into_response());
            }
        }

        if let Some(service) = service_manager.try_get_service("bootstrap")
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
        if let Some(service) = service_manager.try_get_service("bootstrap")
            && let ServiceStatus::Stopped = service.state.status
        {
            process::exit(0);
        }
    }
}
