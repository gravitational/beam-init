use std::io;

use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot};

use crate::Event;
use beam_init::api::{CreateService, SOCKET_PATH, ServiceStatus};

#[allow(clippy::enum_variant_names)]
pub enum Command {
    CreateService {
        name: String,
        service: CreateService,
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
            status: value.state.status.into(),
        }
    }
}

impl From<crate::services::ServiceStatus> for crate::api::ServiceStatus {
    fn from(value: crate::services::ServiceStatus) -> Self {
        match value {
            crate::services::ServiceStatus::Stopped => ServiceStatus::Stopped,
            crate::services::ServiceStatus::Running { main_pid } => {
                ServiceStatus::Running { main_pid }
            }
            crate::services::ServiceStatus::Frozen { main_pid } => {
                ServiceStatus::Frozen { main_pid }
            }
            crate::services::ServiceStatus::Stopping { main_pid } => {
                ServiceStatus::Stopping { main_pid }
            }
            crate::services::ServiceStatus::Failed(exit_status) => {
                ServiceStatus::Failed(exit_status)
            }
        }
    }
}
