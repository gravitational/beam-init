use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt;
use std::process;
use std::time::Duration;

use axum::Json;
use axum::response::{IntoResponse, Response};
use libc::{SIGCHLD, signalfd_siginfo};
use tokio::sync::oneshot;

use crate::services::{ServiceManager, ServiceStatus};
use beam_init::api;

mod api_impl;
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
    let init_cmd = api_impl::Command::CreateService {
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
    api_impl::bind_api_socket("/run/beam-init", tx_event.clone()).unwrap();

    drop(tx_event);
    let mut service_manager = ServiceManager::new(old_sigmask);
    loop {
        match rx_event.recv().await.unwrap() {
            Event::Signal(info) => service_manager.handle_signal(info),
            Event::Command(cmd, tx) => match cmd {
                api_impl::Command::CreateService { name, service } => {
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
                api_impl::Command::StopService { name } => {
                    service_manager.terminate_service(&name);

                    // FIXME: pick a more principled duration, and potentially perform the kill
                    // below in an async way.
                    tokio::time::sleep(Duration::from_millis(5)).await;

                    service_manager.kill_service(&name);

                    let _ = tx.send(Json(()).into_response());
                }
                api_impl::Command::ShowService { name } => {
                    // FIXME: error handling
                    let service = service_manager.get_service(&name).unwrap();

                    let api_service = crate::api::Service::from(service);
                    let _ = tx.send(Json(api_service).into_response());
                }
                api_impl::Command::ListServices => {
                    let services: BTreeMap<String, crate::api::ServiceStatus> = service_manager
                        .list_services()
                        .map(|(name, status)| (name.to_string(), status.into()))
                        .collect();

                    let _ = tx.send(Json(services).into_response());
                }
            },
        }

        if let Some(service) = service_manager.get_service("bootstrap")
            && let ServiceStatus::Failed(status) = service.state.status
        {
            if let Some(code) = status.code() {
                process::exit(code);
            } else if let Some(signal) = status.signal() {
                // SAFETY: This is always safe
                unsafe { libc::raise(signal) };
            } else {
                process::exit(1);
            }
        }
        if let Some(service) = service_manager.get_service("bootstrap")
            && let ServiceStatus::Stopped = service.state.status
        {
            process::exit(0);
        }
    }
}
