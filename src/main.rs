use std::process::{self, Command};

use libc::{SIGCHLD, WEXITSTATUS, WNOHANG, pid_t, signalfd_siginfo, waitpid};

use crate::system::cerr;

mod api;
mod signal_stream;
mod system;

enum Event {
    Command(api::Command),
    Signal(signalfd_siginfo),
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("Starting beam-init");

    // FIXME what is a reasonable channel capacity?
    let (tx_event, mut rx_event) = tokio::sync::mpsc::channel(10);

    // Queue a fake API command to start the first service
    let mut args = std::env::args().skip(1);
    let init_cmd = api::Command::CreateService(
        "bootstrap".to_owned(),
        api::CreateService {
            cmd: args.next().unwrap(),
            args: args.collect(),
        },
    );
    // The channel is empty, so sending always succeeds.
    tx_event.try_send(Event::Command(init_cmd)).unwrap();

    // Listen for SIGCHLD signals
    let old_sigmask = unsafe { signal_stream::init(&[SIGCHLD], tx_event.clone()) }.unwrap();

    // Listen for API commands
    api::bind_api_socket("/run/beam-init", tx_event.clone()).unwrap();

    drop(tx_event);
    let mut init_pid = None;
    loop {
        match rx_event.recv().await.unwrap() {
            Event::Signal(info) => {
                if info.ssi_signo == SIGCHLD as u32 {
                    let mut status = 0;
                    if cerr(unsafe { waitpid(info.ssi_pid as pid_t, &mut status, WNOHANG) })
                        .unwrap()
                        == 0
                    {
                        continue;
                    }

                    if info.ssi_pid == init_pid.unwrap() {
                        process::exit(WEXITSTATUS(status));
                    }
                }
            }
            Event::Command(cmd) => match cmd {
                api::Command::CreateService(name, api::CreateService { cmd, args }) => {
                    println!("Starting service {name}");
                    let mut cmd = Command::new(cmd);
                    cmd.args(args);
                    old_sigmask.with_restored_sigmask(&mut cmd);

                    // We respond to SIGCHLD to reap zombie processes
                    #[expect(clippy::zombie_processes)]
                    let child = cmd.spawn().unwrap();

                    init_pid.get_or_insert(child.id());
                }
            },
        }
    }
}
