use std::collections::BTreeMap;
use std::io;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::StreamExt;

use crate::Event;
use crate::services::{self, ServiceError, ServiceManager, StartReason};
use beam_init::api::{CreateService, SOCKET_PATH, ServiceStatus};

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

pub fn bind_api_socket(tx_event: mpsc::Sender<Event>) -> io::Result<()> {
    let socket = UnixListener::bind(SOCKET_PATH)?;

    let router = Router::new()
        .route("/services", get(list_services))
        .route("/service/{name}", post(create_service))
        .route("/service/{name}/restart", post(restart_service))
        .route("/service/{name}/stop", post(stop_service))
        .route("/service/{name}/freeze", post(freeze_service))
        .route("/service/{name}/thaw", post(thaw_service))
        // .route("/service/{name}/start", post(start_service))
        .route("/service/{name}/show", post(show_service))
        .route("/service/{name}/logs", get(service_logs))
        .with_state(tx_event);

    tokio::spawn(async move {
        axum::serve(socket, router)
            .await
            .expect("axum::serve is documented as never returning an error");
    });

    Ok(())
}

async fn create_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
    Json(service): Json<CreateService>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command(Command::CreateService { name, service }, tx))
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn stop_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command(Command::StopService { name }, tx))
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn restart_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command(Command::RestartService { name }, tx))
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn freeze_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command(Command::FreezeService { name }, tx))
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn thaw_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command(Command::ThawService { name }, tx))
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn show_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command(Command::ShowService { name }, tx))
        .await
        .expect("main task crashed");
    rx.await.expect("main task crashed")
}

async fn list_services(State(tx_events): State<mpsc::Sender<Event>>) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command(Command::ListServices, tx))
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
    query: Query<ServiceLogsQuery>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command(
            Command::ServiceLogs {
                name,
                follow: query.follow,
            },
            tx,
        ))
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
            crate::services::ServiceStatus::Running { main_pid } => {
                ServiceStatus::Running { main_pid }
            }
            crate::services::ServiceStatus::Frozen { main_pid } => {
                ServiceStatus::Frozen { main_pid }
            }
            crate::services::ServiceStatus::Restarting { main_pid, ref name } => {
                ServiceStatus::Restarting {
                    main_pid,
                    name: name.to_owned(),
                }
            }
            crate::services::ServiceStatus::Stopping { main_pid } => {
                ServiceStatus::Stopping { main_pid }
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
    name: &str,
) -> Result<(), ServiceError> {
    service_manager.terminate_service(name)?;

    // FIXME: pick a more principled duration, and potentially perform the kill
    // below in an async way.
    tokio::time::sleep(Duration::from_millis(5)).await;

    service_manager.kill_service(name)
}

pub async fn automatic_restart(
    service_manager: &mut ServiceManager,
    name: &str,
) -> Result<(), ServiceError> {
    service_manager.terminate_restart_service(name)?;

    // FIXME: pick a more principled duration, and potentially perform the kill
    // below in an async way.
    tokio::time::sleep(Duration::from_millis(5)).await;

    service_manager.kill_restart_service(name)
}

pub async fn handle_api_command(
    service_manager: &mut ServiceManager,
    cmd: Command,
) -> Result<Response<Body>, ServiceError> {
    match cmd {
        Command::CreateService { name, service } => {
            let CreateService {
                cmd,
                args,
                readiness,
            } = &service;

            service_manager.create_service(
                name.clone(),
                services::ServiceConfig {
                    cmd: cmd.clone(),
                    args: args.clone(),
                    readiness: readiness.clone(),
                },
            )?;
            service_manager.start_service(&name, StartReason::User)?;
            Ok(Json(service).into_response())
        }
        Command::RestartService { name } => {
            let () = stop_service_cmd(service_manager, &name).await?;
            service_manager.start_service(&name, StartReason::User)?;

            Ok(Json(()).into_response())
        }
        Command::StopService { name } => {
            let () = stop_service_cmd(service_manager, &name).await?;

            Ok(Json(()).into_response())
        }
        Command::FreezeService { name } => {
            service_manager.freeze_service(&name)?;

            Ok(Json(()).into_response())
        }
        Command::ThawService { name } => {
            service_manager.thaw_service(&name)?;

            Ok(Json(()).into_response())
        }
        Command::ShowService { name } => {
            let service = service_manager.get_service(&name)?;

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
                let stream = service_manager.log_reader(&name)?;
                Ok(Response::builder()
                    .header(axum::http::header::CONTENT_TYPE, "text/plain")
                    .body(Body::from_stream(stream.map(|mut line| {
                        line.push('\n');
                        Ok::<_, String>(Bytes::copy_from_slice(line.as_bytes()))
                    })))
                    .expect("valid headers should be set"))
            } else {
                let logs = service_manager.copy_logs(&name).await?;
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
