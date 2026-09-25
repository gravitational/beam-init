use std::io;
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;

use tokio::io::{AsyncReadExt, Interest};
use tokio::net::UnixListener;

use beam_init::system::unix_socket::socket_send_fd;
use beam_init_api::PTY_SOCKET_PATH;
use tokio::sync::{mpsc, oneshot};

use crate::Event;
use crate::api_impl::Credentials;

pub(crate) fn init(tx_event: mpsc::Sender<Event>) -> io::Result<()> {
    let socket = UnixListener::bind(PTY_SOCKET_PATH)?;
    let permissions = std::fs::Permissions::from_mode(0o666);
    std::fs::set_permissions(PTY_SOCKET_PATH, permissions)?;

    tokio::spawn(async move {
        loop {
            match socket.accept().await {
                Ok((mut stream, _addr)) => {
                    let tx_event2 = tx_event.clone();
                    tokio::spawn(async move {
                        let credentials = match stream.peer_cred() {
                            Ok(cred) => Credentials {
                                uid: cred.uid(),
                                gid: cred.gid(),
                            },
                            Err(err) => {
                                eprintln!("No Unix peer credentials: {err}");
                                return;
                            }
                        };

                        let name_len = match stream.read_u8().await {
                            Ok(id) => usize::from(id),
                            Err(err) => {
                                eprintln!("Failed to read service name length from client: {err}");
                                return;
                            }
                        };
                        let mut name = vec![0; name_len];
                        match stream.read_exact(&mut name).await {
                            Ok(_) => {}
                            Err(err) => {
                                eprintln!("Failed to read service name from client: {err}");
                                return;
                            }
                        };
                        let name = match String::from_utf8(name) {
                            Ok(name) => name,
                            Err(err) => {
                                eprintln!("Service name is not UTF-8: {err}");
                                return;
                            }
                        };

                        let (res_tx, res_rx) = oneshot::channel();
                        tx_event2
                            .send(Event::GetPty {
                                name,
                                tx: res_tx,
                                credentials,
                            })
                            .await
                            .expect("main task crashed");

                        let fd = match res_rx.await {
                            Ok(fd) => fd,
                            Err(_) => {
                                // An error happened, already reported by the main task
                                return;
                            }
                        };

                        let res = stream
                            .async_io(Interest::WRITABLE, || {
                                socket_send_fd(&stream, &[0], fd.0.as_fd())
                            })
                            .await;
                        if let Err(err) = res {
                            eprintln!("Failed to send fd to client: {err}");
                        }
                        let res = stream
                            .async_io(Interest::WRITABLE, || {
                                socket_send_fd(&stream, &[0], fd.1.as_fd())
                            })
                            .await;
                        if let Err(err) = res {
                            eprintln!("Failed to send fd to client: {err}");
                        }

                        // FIXME remove event pipe on close
                    });
                }
                Err(err) => eprintln!("Failed to accept fd socket connection: {err}"),
            }
        }
    });

    Ok(())
}
