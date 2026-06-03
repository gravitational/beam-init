use std::os::unix::process::ExitStatusExt;
use std::process;

use axum::response::Response;
use clap::Parser;
use libc::{SIGCHLD, signalfd_siginfo};
use tokio::sync::oneshot;

use crate::services::{ServiceManager, ServiceStatus};
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

#[derive(Parser)]
#[command(name = "beam-init", trailing_var_arg = true)]
struct Cli {
    #[arg(long, value_name = "SCRIPT", value_hint = clap::ValueHint::FilePath)]
    init_script: Option<String>,

    #[arg(required = true, num_args = 1.., value_hint = clap::ValueHint::CommandWithArguments)]
    command: Vec<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("Starting beam-init");

    let args = Cli::parse();

    // FIXME what is a reasonable channel capacity?
    let (tx_event, mut rx_event) = tokio::sync::mpsc::channel(10);

    let queue_start_service = |name, cmd, args| {
        let cmd = api_impl::Command::CreateService {
            name,
            service: api::CreateService { cmd, args },
        };
        tx_event
            .try_send(Event::Command(cmd, oneshot::channel().0))
            .expect("channel should have capacity for startup commands");
    };

    if let Some(init_script) = args.init_script {
        queue_start_service("init-script".to_owned(), init_script, Vec::new());
    }
    let mut bootstrap_command = args.command.into_iter();
    let bootstrap_cmd = bootstrap_command.next().expect("required by clap");
    queue_start_service(
        "bootstrap".to_owned(),
        bootstrap_cmd,
        bootstrap_command.collect(),
    );

    // Listen for SIGCHLD signals
    let old_sigmask = signal_stream::init(&[SIGCHLD], tx_event.clone())
        .expect("failed to initialize the signal stream");

    // Listen for API commands
    api_impl::bind_api_socket(tx_event).expect("failed to bind api socket");

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
                let _ = tx.send(res);
            }
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
