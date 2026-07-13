use std::collections::BTreeMap;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::{Arc, Mutex};
use std::{fmt, io};

use beam_init::api::FD_SOCKET_PATH;
use tokio::io::{AsyncReadExt, Interest};
use tokio::net::UnixListener;

use crate::system::unix_socket::socket_send_fd;

pub struct StoredFd {
    id: u64,
    store: Arc<Mutex<FdStoreInner>>,
}

impl fmt::Debug for StoredFd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredFd").field("id", &self.id).finish()
    }
}

impl StoredFd {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn get(&self) -> Arc<OwnedFd> {
        Arc::clone(&self.store.lock().expect("lock shouldn't be poisoned").fds[&self.id])
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
    // FIXME store user id once unprivileged users can start services
    fds: BTreeMap<u64, Arc<OwnedFd>>,
    next_id: u64,
}

impl FdStore {
    pub(crate) fn no_socket() -> Self {
        FdStore(Arc::new(Mutex::new(FdStoreInner::default())))
    }

    pub(crate) fn bind_socket() -> io::Result<Self> {
        let socket = UnixListener::bind(FD_SOCKET_PATH)?;
        let inner = Arc::new(Mutex::new(FdStoreInner::default()));

        let inner2 = inner.clone();
        tokio::spawn(async move {
            loop {
                match socket.accept().await {
                    Ok((mut stream, _addr)) => {
                        let inner3 = inner2.clone();
                        tokio::spawn(async move {
                            let id = match stream.read_u64_le().await {
                                Ok(id) => id,
                                Err(err) => {
                                    eprintln!("Failed to read fdstore id from client: {err}");
                                    return;
                                }
                            };
                            let fd = Arc::clone(
                                &inner3.lock().expect("lock shouldn't be poisoned").fds[&id],
                            );
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

    pub(crate) fn add(&self, fd: OwnedFd) -> StoredFd {
        let mut this = self.0.lock().expect("lock shouldn't be poisoned");

        let id = this.next_id;
        assert!(this.fds.insert(id, Arc::new(fd)).is_none());
        this.next_id += 1;

        StoredFd {
            id,
            store: self.0.clone(),
        }
    }
}
