use std::process::{self, Command};

use libc::{SIGCHLD, WEXITSTATUS, WNOHANG, pid_t, waitpid};

use crate::system::cerr;

mod signal_stream;
mod system;

fn main() {
    println!("Starting dumb-init");

    let mut signals = unsafe { signal_stream::init(&[SIGCHLD]) }.unwrap();

    let mut args = std::env::args().skip(1);
    let mut cmd = Command::new(args.next().unwrap());
    cmd.args(args);
    signals.with_restored_sigmask(&mut cmd);
    let child = cmd.spawn().unwrap();
    let pid = child.id();

    loop {
        let info = signals.recv().unwrap();
        if info.ssi_signo == SIGCHLD as _ {
            let mut status = 0;
            if cerr(unsafe { waitpid(info.ssi_pid as pid_t, &mut status, WNOHANG) }).unwrap() == 0 {
                continue;
            }

            if info.ssi_pid == pid {
                process::exit(WEXITSTATUS(status));
            }
        }
    }
}
