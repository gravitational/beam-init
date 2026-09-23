use std::collections::BTreeMap;
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::{fmt, io};

use beam_init::system::pty::{Pty, PtyClient};
use libc::uid_t;
use tokio::io::{AsyncReadExt, Interest};
use tokio::net::UnixListener;

use beam_init::system::unix_socket::socket_send_fd;
use beam_init_api::PTY_SOCKET_PATH;

pub struct StoredPty {
    id: u64,
    pty: Arc<Pty>,
    store: Arc<Mutex<PtyStoreInner>>,
}

impl fmt::Debug for StoredPty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredFd")
            .field("id", &self.id)
            .field("fd", &self.pty)
            .finish()
    }
}

impl StoredPty {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn path(&self) -> &Path {
        &self.pty.path
    }

    pub fn client(&self) -> PtyClient<'_> {
        self.pty.client()
    }
}

impl Drop for StoredPty {
    fn drop(&mut self) {
        self.store
            .lock()
            .expect("lock shouldn't be poisoned")
            .fds
            .remove(&self.id)
            .expect("fd got removed twice");
    }
}

#[derive(Debug)]
// NOTE: This uses a sync lock to allow locking outside of async functions. As
// such the critical section must be as short as possible and may not span an
// await to prevent deadlocks.
pub struct PtyStore(Arc<Mutex<PtyStoreInner>>);

#[derive(Debug, Default)]
struct PtyStoreInner {
    fds: BTreeMap<u64, (Arc<Pty>, uid_t)>,
    next_id: u64,
}

impl PtyStore {
    pub(crate) fn no_socket() -> Self {
        PtyStore(Arc::new(Mutex::new(PtyStoreInner::default())))
    }

    pub(crate) fn bind_socket() -> io::Result<Self> {
        let socket = UnixListener::bind(PTY_SOCKET_PATH)?;
        let permissions = std::fs::Permissions::from_mode(0o666);
        std::fs::set_permissions(PTY_SOCKET_PATH, permissions)?;

        let inner = Arc::new(Mutex::new(PtyStoreInner::default()));

        let inner2 = inner.clone();
        tokio::spawn(async move {
            loop {
                match socket.accept().await {
                    Ok((mut stream, _addr)) => {
                        let inner3 = inner2.clone();
                        tokio::spawn(async move {
                            let client_uid = match stream.peer_cred() {
                                Ok(cred) => cred.uid(),
                                Err(err) => {
                                    eprintln!("No Unix peer credentials: {err}");
                                    return;
                                }
                            };
                            let id = match stream.read_u64_le().await {
                                Ok(id) => id,
                                Err(err) => {
                                    eprintln!("Failed to read ptystore id from client: {err}");
                                    return;
                                }
                            };

                            let res = inner3
                                .lock()
                                .expect("lock shouldn't be poisoned")
                                .fds
                                .get(&id)
                                .map(|(fd, uid)| (Arc::clone(fd), *uid));
                            let Some((fd, owner_uid)) = res else {
                                eprintln!("Client requested non-existent fd");
                                return;
                            };
                            if client_uid != 0 && client_uid != owner_uid {
                                eprintln!("Client requested fd for different user");
                                return;
                            }

                            let res = stream
                                .async_io(Interest::WRITABLE, || {
                                    socket_send_fd(&stream, &[0], fd.as_fd())
                                })
                                .await;
                            if let Err(err) = res {
                                eprintln!("Failed to send fd to client: {err}");
                            }
                        });
                    }
                    Err(err) => eprintln!("Failed to accept fd socket connection: {err}"),
                }
            }
        });

        Ok(PtyStore(inner))
    }

    pub(crate) fn add_pty(&self, fd: Pty, uid: uid_t) -> StoredPty {
        let fd = Arc::new(fd);

        let mut this = self.0.lock().expect("lock shouldn't be poisoned");

        let id = this.next_id;
        assert!(this.fds.insert(id, (fd.clone(), uid)).is_none());
        this.next_id += 1;

        StoredPty {
            id,
            pty: fd,
            store: self.0.clone(),
        }
    }
}
