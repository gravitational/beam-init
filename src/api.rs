use std::io;
use std::path::Path;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::Event;

pub enum Command {
    StartService { cmd: String, args: Vec<String> },
}

pub fn bind_api_socket(path: impl AsRef<Path>, tx_event: mpsc::Sender<Event>) -> io::Result<()> {
    let socket = UnixListener::bind(path)?;

    tokio::spawn(async move {
        loop {
            let stream = socket.accept().await.unwrap().0;
            tokio::spawn(accept(stream, tx_event.clone()));
        }
    });

    Ok(())
}

async fn accept(stream: UnixStream, tx_event: mpsc::Sender<Event>) {
    let mut lines = BufReader::new(stream).lines();

    while let Some(line) = lines.next_line().await.unwrap() {
        let mut args = line.split(" ").map(|arg| arg.to_owned());
        if tx_event
            .send(Event::Command(Command::StartService {
                cmd: args.next().unwrap(),
                args: args.collect(),
            }))
            .await
            .is_err()
        {
            return; // Main event loop has finished
        }
    }
}
