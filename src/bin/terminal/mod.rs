use std::io;
use std::os::fd::OwnedFd;
use tokio::fs::File;

use tokio::runtime::LocalRuntime as Runtime;

pub(super) fn manage(pty: OwnedFd) -> io::Result<()> {
    Runtime::new()
        .expect("to make a tokio runtime")
        .block_on(attach(pty))
}

async fn attach(pty: OwnedFd) -> io::Result<()> {
    let mut app_r = File::from(pty.try_clone().expect("to dup a file descriptor"));
    let mut app_w = File::from(pty);

    let mut source = tokio::io::stdin();
    let mut sink = tokio::io::stdout();

    let left_to_right = tokio::io::copy(&mut app_r, &mut sink);
    let right_to_left = tokio::io::copy(&mut source, &mut app_w);

    tokio::select! {
        _ = right_to_left => {
            ()
        },
        _ = left_to_right => {
            ()
        },
    }

    Ok(())
}
