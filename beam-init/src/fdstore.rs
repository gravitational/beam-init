use std::collections::BTreeMap;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::{fmt, io};

use libc::uid_t;
use tokio::io::{AsyncReadExt, Interest};
use tokio::net::UnixListener;

use beam_init::system::unix_socket::socket_send_fd;
use beam_init_api::FD_SOCKET_PATH;

pub struct StoredFd {
    id: u64,
    fd: Arc<OwnedFd>,
    store: Arc<Mutex<FdStoreInner>>,
}

impl fmt::Debug for StoredFd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredFd")
            .field("id", &self.id)
            .field("fd", &self.fd)
            .finish()
    }
}

impl StoredFd {
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl AsFd for StoredFd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

impl Drop for StoredFd {
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
pub struct FdStore(Arc<Mutex<FdStoreInner>>);

#[derive(Debug, Default)]
struct FdStoreInner {
    fds: BTreeMap<u64, (Arc<OwnedFd>, uid_t)>,
    next_id: u64,
}

impl FdStore {
    pub(crate) fn no_socket() -> Self {
        FdStore(Arc::new(Mutex::new(FdStoreInner::default())))
    }

    pub(crate) fn bind_socket() -> io::Result<Self> {
        let socket = UnixListener::bind(FD_SOCKET_PATH)?;
        let permissions = std::fs::Permissions::from_mode(0o666);
        std::fs::set_permissions(FD_SOCKET_PATH, permissions)?;

        let inner = Arc::new(Mutex::new(FdStoreInner::default()));

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
                                    eprintln!("Failed to read fdstore id from client: {err}");
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

        Ok(FdStore(inner))
    }

    pub(crate) fn add(&self, fd: OwnedFd, uid: uid_t) -> StoredFd {
        let fd = Arc::new(fd);

        let mut this = self.0.lock().expect("lock shouldn't be poisoned");

        let id = this.next_id;
        assert!(this.fds.insert(id, (fd.clone(), uid)).is_none());
        this.next_id += 1;

        StoredFd {
            id,
            fd,
            store: self.0.clone(),
        }
    }
}
