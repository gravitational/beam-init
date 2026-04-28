use std::process;

use axum::Json;
use axum::response::{IntoResponse, Response};
use libc::{SIGCHLD, signalfd_siginfo};
use tokio::sync::oneshot;

use crate::services::{ServiceManager, ServiceStatus};

mod api;
mod services;
mod signal_stream;
mod system;

enum Event {
    Command(api::Command, oneshot::Sender<Response>),
    Signal(signalfd_siginfo),
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("Starting beam-init");

    // FIXME what is a reasonable channel capacity?
    let (tx_event, mut rx_event) = tokio::sync::mpsc::channel(10);

    // Queue a fake API command to start the first service
    let mut args = std::env::args().skip(1);
    let init_cmd = api::Command::CreateService {
        name: "bootstrap".to_owned(),
        service: api::CreateService {
            cmd: args.next().unwrap(),
            args: args.collect(),
        },
    };
    // The channel is empty, so sending always succeeds.
    tx_event
        .try_send(Event::Command(init_cmd, oneshot::channel().0))
        .unwrap();

    // Listen for SIGCHLD signals
    let old_sigmask = signal_stream::init(&[SIGCHLD], tx_event.clone()).unwrap();

    // Listen for API commands
    api::bind_api_socket("/run/beam-init", tx_event.clone()).unwrap();

    drop(tx_event);
    let mut service_manager = ServiceManager::new(old_sigmask);
    loop {
        match rx_event.recv().await.unwrap() {
            Event::Signal(info) => service_manager.handle_signal(info),
            Event::Command(cmd, tx) => match cmd {
                api::Command::CreateService { name, service } => {
                    service_manager.create_service(
                        name.clone(),
                        services::ServiceConfig {
                            cmd: service.cmd.clone(),
                            args: service.args.clone(),
                        },
                    );
                    service_manager.start_service(&name);
                    let _ = tx.send(Json(service).into_response());
                }
            },
        }

        if let Some(service) = service_manager.get_service("bootstrap")
            && let ServiceStatus::Failed(status) = service.state.status
        {
            // FIXME exit with signal if child exited with signal
            process::exit(status.code().unwrap());
        }
    }
}
