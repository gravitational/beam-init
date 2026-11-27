use std::collections::BTreeMap;
use std::ffi::c_int;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::pin::Pin;

use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use beam_init::BOOTSTRAP_NAME;
use futures_core::Stream;
use serde::Deserialize;
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::Event;
use crate::environment::build_environment;
use crate::services::service_status_publisher::ServiceStatusSubscription;
use crate::services::{
    self, AfterStop, ServiceError, ServiceManager, StartReason, TerminationToken,
};
use beam_init::system::cerr;
use beam_init_api::{API_SOCKET_PATH, CreateService, ServiceStatus, VersionResponse};

pub type Reply<T> = oneshot::Sender<Result<T, ServiceError>>;

pub struct LogStream(Pin<Box<dyn Stream<Item = String> + Send>>);

impl std::fmt::Debug for LogStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LogStream").finish_non_exhaustive()
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug)]
pub enum Command {
    CreateService {
        name: String,
        service: CreateService,
        reply: Reply<CreateService>,
    },
    RestartService {
        name: String,
        reply: Reply<()>,
    },
    StopService {
        name: String,
        prune: bool,
        reply: Reply<()>,
    },
    FreezeService {
        name: String,
        reply: Reply<()>,
    },
    ThawService {
        name: String,
        reply: Reply<()>,
    },
    ShowService {
        name: String,
        reply: Reply<beam_init_api::Service>,
    },
    ListServices {
        reply: Reply<BTreeMap<String, beam_init_api::ServiceStatus>>,
    },
    ServiceLogs {
        name: String,
        reply: Reply<Vec<String>>,
    },
    FollowLogs {
        name: String,
        reply: Reply<LogStream>,
    },
    NotifyPtyAttached {
        name: String,
        reply: Reply<()>,
    },
    NotifyPtyDetached {
        name: String,
        reply: Reply<()>,
    },
    SendSignal {
        name: String,
        sig: c_int,
        reply: Reply<()>,
    },
    ForceKillService {
        name: String,
        token: TerminationToken,
        reply: Reply<()>,
    },
    WaitService {
        name: String,
        reply: Reply<ServiceStatusSubscription>,
    },
}

use axum::serve::IncomingStream;

#[derive(Debug, Clone, Copy)]
pub struct Credentials {
    pub uid: libc::uid_t,
    pub gid: libc::gid_t,
}

impl Credentials {
    pub fn root() -> Self {
        Self { uid: 0, gid: 0 }
    }

    /// Set the process credentials from `self``.
    pub fn set_creds(self) -> io::Result<()> {
        let Credentials { uid, gid } = self;

        // Set the supplementary group IDs for the child to the empty list.
        // SAFETY: a valid empty list is passed to setgroups
        unsafe { cerr(libc::setgroups(0, std::ptr::null())) }?;

        // Set the group and user ID.
        // SAFETY: setgid and setsid are safe to call
        unsafe {
            cerr(libc::setgid(gid))?;
            cerr(libc::setuid(uid))?;
        }

        Ok(())
    }
}

impl axum::extract::connect_info::Connected<IncomingStream<'_, UnixListener>> for Credentials {
    fn connect_info(stream: IncomingStream<'_, UnixListener>) -> Self {
        let cred = stream.io().peer_cred().expect("no Unix peer credentials");
        Credentials {
            uid: cred.uid(),
            gid: cred.gid(),
        }
    }
}

pub fn bind_api_socket(
    tx_event: mpsc::Sender<Event>,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let socket = UnixListener::bind(API_SOCKET_PATH)?;

    // Allow all users to read from/write to this socket.
    let permissions = std::fs::Permissions::from_mode(0o666);
    std::fs::set_permissions(API_SOCKET_PATH, permissions)?;

    let router = Router::new()
        .route("/services", get(list_services))
        .route(
            "/service/{name}",
            post(create_service).delete(delete_service),
        )
        .route("/service/{name}/restart", post(restart_service))
        .route("/service/{name}/stop", post(stop_service))
        .route("/service/{name}/freeze", post(freeze_service))
        .route("/service/{name}/thaw", post(thaw_service))
        .route("/service/{name}/show", post(show_service))
        .route("/service/{name}/logs", get(service_logs))
        .route(
            "/service/{name}/notify_pty_attached",
            post(notify_pty_attached),
        )
        .route(
            "/service/{name}/notify_pty_detached",
            post(notify_pty_detached),
        )
        .route("/service/{name}/send_signal", post(send_signal))
        .route("/service/{name}/wait", get(wait_service))
        .route("/version", get(version))
        .fallback(fallback_route)
        .with_state(tx_event);

    tokio::spawn(async move {
        axum::serve(
            socket,
            router.into_make_service_with_connect_info::<Credentials>(),
        )
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await
        .expect("axum::serve is documented as never returning an error");
    });

    Ok(())
}

async fn fallback_route() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "404 Not Found")
}

async fn create_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
    Json(service): Json<CreateService>,
) -> Result<Json<CreateService>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::CreateService {
                name,
                service,
                reply: tx,
            },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

async fn stop_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Result<Json<()>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::StopService {
                name,
                prune: false,
                reply: tx,
            },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

async fn delete_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Result<Json<()>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::StopService {
                name,
                prune: true,
                reply: tx,
            },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

async fn restart_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Result<Json<()>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::RestartService { name, reply: tx },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

async fn freeze_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Result<Json<()>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::FreezeService { name, reply: tx },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

async fn thaw_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Result<Json<()>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::ThawService { name, reply: tx },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

async fn show_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Result<Json<beam_init_api::Service>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::ShowService { name, reply: tx },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

async fn list_services(
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Result<Json<BTreeMap<String, beam_init_api::ServiceStatus>>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::ListServices { reply: tx },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

#[derive(Deserialize)]
struct ServiceLogsQuery {
    #[serde(default)]
    follow: bool,
}

async fn service_logs(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
    query: Query<ServiceLogsQuery>,
) -> Result<Response, ServiceError> {
    if query.follow {
        let (tx, rx) = oneshot::channel();
        let cmd = Command::FollowLogs { name, reply: tx };
        tx_events
            .send(Event::Command {
                command: cmd,
                credentials,
            })
            .await
            .expect("main task crashed");
        let LogStream(stream) = rx.await.expect("main task crashed")?;
        let body = Body::from_stream(stream.map(|mut line| {
            line.push('\n');
            Ok::<_, std::convert::Infallible>(line)
        }));

        Ok(([(axum::http::header::CONTENT_TYPE, "text/plain")], body).into_response())
    } else {
        let (tx, rx) = oneshot::channel();
        let cmd = Command::ServiceLogs { name, reply: tx };
        tx_events
            .send(Event::Command {
                command: cmd,
                credentials,
            })
            .await
            .expect("main task crashed");
        let logs = rx.await.expect("main task crashed")?;
        Ok(logs
            .into_iter()
            .map(|mut line| {
                line.push('\n');
                line
            })
            .collect::<String>()
            .into_response())
    }
}

async fn notify_pty_attached(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Result<Json<()>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::NotifyPtyAttached { name, reply: tx },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

async fn notify_pty_detached(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Result<Json<()>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::NotifyPtyDetached { name, reply: tx },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

async fn send_signal(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
    Json(sig): Json<c_int>,
) -> Result<Json<()>, ServiceError> {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::SendSignal {
                name,
                sig,
                reply: tx,
            },
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed").map(Json)
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) struct WaitConditionQuery {
    state: beam_init_api::ServiceStatusKind,
}

async fn wait_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
    Query(WaitConditionQuery { state: status_kind }): Query<WaitConditionQuery>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::WaitService { name, reply: tx },
            credentials,
        })
        .await
        .expect("main task crashed");
    let mut subscription = match rx.await.expect("main task crashed") {
        Ok(s) => s,
        Err(err) => return err.into_response(),
    };
    loop {
        match subscription.recv().await {
            Ok(s) if s.kind() == status_kind => return Json(s).into_response(),
            Ok(_) => {}
            // Error here means either the service was pruned or we lagged on reading events
            Err(err) => match err {
                tokio::sync::broadcast::error::RecvError::Closed => {
                    return ServiceError::ServicePrunedWhileWaiting.into_response();
                }
                tokio::sync::broadcast::error::RecvError::Lagged(skipped) => {
                    return ServiceError::ObservationLagged { skipped }.into_response();
                }
            },
        }
    }
}

async fn version() -> Response {
    Json(VersionResponse {
        version: crate::VERSION.to_string(),
        sha: crate::GIT_SHA.to_string(),
    })
    .into_response()
}

impl From<&crate::services::Service> for beam_init_api::Service {
    fn from(value: &crate::services::Service) -> Self {
        Self {
            cmd: value.config.cmd.clone(),
            args: value.config.args.clone(),
            status: (&value.state.status).into(),
            automatic_restart_attempts: value.state.automatic_restart_attempts,
            labels: value.config.labels.clone(),
        }
    }
}

impl From<&crate::services::ServiceStatus> for beam_init_api::ServiceStatus {
    fn from(value: &crate::services::ServiceStatus) -> Self {
        match value {
            crate::services::ServiceStatus::Stopped => ServiceStatus::Stopped,
            crate::services::ServiceStatus::Running { process, pty } => ServiceStatus::Running {
                main_pid: process.main_pid,
                pty: pty.as_ref().map(|pty| pty.path.clone()),
            },
            crate::services::ServiceStatus::Frozen { process, pty } => ServiceStatus::Frozen {
                main_pid: process.main_pid,
                pty: pty.as_ref().map(|pty| pty.path.clone()),
            },
            crate::services::ServiceStatus::Stopping {
                process,
                after_stop,
                pending_kill: _,
            } => ServiceStatus::Stopping {
                main_pid: process.main_pid,
                prune: matches!(after_stop, services::AfterStop::Prune),
                will_restart: matches!(after_stop, services::AfterStop::Restart(_)),
            },

            crate::services::ServiceStatus::Exited(exit_status) => {
                ServiceStatus::Exited((*exit_status).into())
            }
            crate::services::ServiceStatus::Error(err) => ServiceStatus::Error(err.to_string()),
            crate::services::ServiceStatus::Dummy => unreachable!(),
        }
    }
}

pub async fn stop_service_cmd(
    service_manager: &mut ServiceManager,
    credentials: Credentials,
    name: &str,
    after_stop: AfterStop,
) -> Result<(), ServiceError> {
    let service = service_manager.get_service(credentials, name)?;

    // We need to reset credentials here to the service's credentials as automatic restart
    // will send root credentials.
    let credentials = service.config.credentials;

    service_manager.terminate_service(credentials, name, after_stop)
}

pub async fn handle_api_command(
    service_manager: &mut ServiceManager,
    cmd: Command,
    credentials: Credentials,
) {
    match cmd {
        Command::CreateService {
            name,
            service,
            reply,
        } => {
            let CreateService {
                cmd,
                args,
                env,
                liveness,
                pty,
                labels,
                graceful_termination_period,
            } = &service;
            let env = if name == BOOTSTRAP_NAME {
                std::env::vars_os().collect()
            } else {
                let mut svc_env =
                    match build_environment(credentials.uid, service_manager.user_env_files())
                        .map_err(|e| ServiceError::BuildEnvironment { err: e.to_string() })
                    {
                        Ok(svc_env) => svc_env,
                        Err(e) => {
                            let _ = reply.send(Err(e));
                            return;
                        }
                    };
                svc_env.extend(env.iter().map(|(k, v)| (k.into(), v.into())));
                svc_env
            };

            if let Err(e) = service_manager.create_service(
                name.clone(),
                services::ServiceConfig {
                    cmd: cmd.clone(),
                    args: args.clone(),
                    env,
                    liveness: liveness.clone(),
                    pty: *pty,
                    credentials,
                    labels: labels.clone(),
                    graceful_termination_period: *graceful_termination_period,
                },
            ) {
                let _ = reply.send(Err(e));
                return;
            };
            if let Err(e) = service_manager.start_service(credentials, &name, StartReason::User) {
                let _ = reply.send(Err(e));
                return;
            };
            let _ = reply.send(Ok(service));
        }
        Command::RestartService { name, reply } => {
            let result = stop_service_cmd(
                service_manager,
                credentials,
                &name,
                AfterStop::Restart(StartReason::User),
            )
            .await;

            let _ = reply.send(result);
        }
        Command::StopService { name, prune, reply } => {
            let after_stop = if prune {
                AfterStop::Prune
            } else {
                AfterStop::Stop
            };
            let result = stop_service_cmd(service_manager, credentials, &name, after_stop).await;

            let _ = reply.send(result);
        }
        Command::FreezeService { name, reply } => {
            let _ = reply.send(service_manager.freeze_service(credentials, &name));
        }
        Command::ThawService { name, reply } => {
            let _ = reply.send(service_manager.thaw_service(credentials, &name));
        }
        Command::ShowService { name, reply } => {
            let service = service_manager
                .get_service(credentials, &name)
                .map(beam_init_api::Service::from);
            let _ = reply.send(service);
        }
        Command::ListServices { reply } => {
            let services: BTreeMap<String, beam_init_api::ServiceStatus> = service_manager
                .list_services(credentials)
                .map(|(name, status)| (name.to_string(), status.into()))
                .collect();

            let _ = reply.send(Ok(services));
        }
        Command::ServiceLogs { name, reply } => {
            let logs = service_manager.copy_logs(credentials, &name).await;
            let _ = reply.send(logs);
        }
        Command::FollowLogs { name, reply } => {
            let stream = service_manager
                .log_reader(credentials, &name)
                .map(|stream| LogStream(Box::pin(stream)));
            let _ = reply.send(stream);
        }
        Command::NotifyPtyAttached { name, reply } => {
            let _ = reply.send(service_manager.notify_pty_attached(credentials, &name));
        }
        Command::NotifyPtyDetached { name, reply } => {
            let _ = reply.send(service_manager.notify_pty_detached(credentials, &name));
        }
        Command::SendSignal { name, sig, reply } => {
            let _ = reply.send(service_manager.send_signal(credentials, &name, sig));
        }
        Command::ForceKillService { name, token, reply } => {
            service_manager.force_kill_service(&name, token);
            let _ = reply.send(Ok(()));
        }
        Command::WaitService { name, reply: tx } => {
            let _ = tx.send(service_manager.subscribe(credentials, &name));
        }
    }
}
