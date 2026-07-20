use std::collections::BTreeMap;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::StreamExt;

use crate::Event;
use crate::services::{self, ServiceError, ServiceManager, StartReason};
use beam_init::api::{API_SOCKET_PATH, CreateService, ServiceStatus};

#[allow(clippy::enum_variant_names)]
pub enum Command {
    CreateService {
        name: String,
        service: CreateService,
    },
    RestartService {
        name: String,
    },
    StopService {
        name: String,
        prune: bool,
    },
    FreezeService {
        name: String,
    },
    ThawService {
        name: String,
    },
    ShowService {
        name: String,
    },
    ListServices,
    ServiceLogs {
        name: String,
        follow: bool,
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

pub fn bind_api_socket(tx_event: mpsc::Sender<Event>) -> io::Result<()> {
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
        .with_state(tx_event);

    tokio::spawn(async move {
        axum::serve(
            socket,
            router.into_make_service_with_connect_info::<Credentials>(),
        )
        .await
        .expect("axum::serve is documented as never returning an error");
    });

    Ok(())
}

async fn create_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
    Json(service): Json<CreateService>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::CreateService { name, service },
            tx,
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn stop_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::StopService { name, prune: false },
            tx,
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn delete_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::StopService { name, prune: true },
            tx,
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn restart_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::RestartService { name },
            tx,
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn freeze_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::FreezeService { name },
            tx,
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn thaw_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::ThawService { name },
            tx,
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn show_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::ShowService { name },
            tx,
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn list_services(
    State(tx_events): State<mpsc::Sender<Event>>,
    ConnectInfo(credentials): ConnectInfo<Credentials>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::ListServices,
            tx,
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
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
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command {
            command: Command::ServiceLogs {
                name,
                follow: query.follow,
            },
            tx,
            credentials,
        })
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

impl From<&crate::services::Service> for crate::api::Service {
    fn from(value: &crate::services::Service) -> Self {
        Self {
            cmd: value.config.cmd.clone(),
            args: value.config.args.clone(),
            status: (&value.state.status).into(),
            automatic_restart_attempts: value.state.automatic_restart_attempts,
        }
    }
}

impl From<&crate::services::ServiceStatus> for crate::api::ServiceStatus {
    fn from(value: &crate::services::ServiceStatus) -> Self {
        match *value {
            crate::services::ServiceStatus::Stopped => ServiceStatus::Stopped,
            crate::services::ServiceStatus::Running { main_pid, ref pty } => {
                ServiceStatus::Running {
                    main_pid,
                    pty: pty
                        .as_ref()
                        .map(|inner| (inner.master.id(), inner.path.clone())),
                }
            }
            crate::services::ServiceStatus::Frozen { main_pid, ref pty } => ServiceStatus::Frozen {
                main_pid,
                pty: pty
                    .as_ref()
                    .map(|inner| (inner.master.id(), inner.path.clone())),
            },
            crate::services::ServiceStatus::Restarting { main_pid, ref name } => {
                ServiceStatus::Restarting {
                    main_pid,
                    name: name.to_owned(),
                }
            }
            crate::services::ServiceStatus::Stopping { main_pid, prune } => {
                ServiceStatus::Stopping { main_pid, prune }
            }
            crate::services::ServiceStatus::Exited(exit_status) => {
                ServiceStatus::Exited(exit_status)
            }
            crate::services::ServiceStatus::Error(ref err) => ServiceStatus::Error(err.to_string()),
        }
    }
}

async fn stop_service_cmd(
    service_manager: &mut ServiceManager,
    credentials: Credentials,
    name: &str,
    prune: bool,
) -> Result<(), ServiceError> {
    service_manager.terminate_service(credentials, name, prune)?;

    // FIXME: pick a more principled duration, and potentially perform the kill
    // below in an async way.
    tokio::time::sleep(Duration::from_millis(5)).await;

    service_manager.kill_service(credentials, name)
}

pub async fn automatic_restart(
    service_manager: &mut ServiceManager,
    name: &str,
) -> Result<(), ServiceError> {
    let service = service_manager.get_service(Credentials::root(), name)?;
    let credentials = service.config.credentials;

    service_manager.terminate_restart_service(credentials, name)?;

    // FIXME: pick a more principled duration, and potentially perform the kill
    // below in an async way.
    tokio::time::sleep(Duration::from_millis(5)).await;

    service_manager.kill_restart_service(credentials, name)
}

pub async fn handle_api_command(
    service_manager: &mut ServiceManager,
    cmd: Command,
    credentials: Credentials,
) -> Result<Response<Body>, ServiceError> {
    match cmd {
        Command::CreateService { name, service } => {
            let CreateService {
                cmd,
                args,
                liveness,
                pty,
            } = &service;

            service_manager.create_service(
                name.clone(),
                services::ServiceConfig {
                    cmd: cmd.clone(),
                    args: args.clone(),
                    liveness: liveness.clone(),
                    pty: *pty,
                    credentials,
                },
            )?;
            service_manager.start_service(credentials, &name, StartReason::User)?;
            Ok(Json(service).into_response())
        }
        Command::RestartService { name } => {
            let prune = false;
            let () = stop_service_cmd(service_manager, credentials, &name, prune).await?;
            service_manager.start_service(credentials, &name, StartReason::User)?;

            Ok(Json(()).into_response())
        }
        Command::StopService { name, prune } => {
            let () = stop_service_cmd(service_manager, credentials, &name, prune).await?;

            Ok(Json(()).into_response())
        }
        Command::FreezeService { name } => {
            service_manager.freeze_service(credentials, &name)?;

            Ok(Json(()).into_response())
        }
        Command::ThawService { name } => {
            service_manager.thaw_service(credentials, &name)?;

            Ok(Json(()).into_response())
        }
        Command::ShowService { name } => {
            let service = service_manager.get_service(credentials, &name)?;

            let api_service = crate::api::Service::from(service);
            Ok(Json(api_service).into_response())
        }
        Command::ListServices => {
            let services: BTreeMap<String, crate::api::ServiceStatus> = service_manager
                .list_services()
                .map(|(name, status)| (name.to_string(), status.into()))
                .collect();

            Ok(Json(services).into_response())
        }
        Command::ServiceLogs { name, follow } => {
            if follow {
                let stream = service_manager.log_reader(credentials, &name)?;
                Ok(Response::builder()
                    .header(axum::http::header::CONTENT_TYPE, "text/plain")
                    .body(Body::from_stream(stream.map(|mut line| {
                        line.push('\n');
                        Ok::<_, String>(Bytes::copy_from_slice(line.as_bytes()))
                    })))
                    .expect("valid headers should be set"))
            } else {
                let logs = service_manager.copy_logs(credentials, &name).await?;
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
    }
}
