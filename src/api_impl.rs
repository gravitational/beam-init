use std::io;

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot};

use crate::Event;
use beam_init::api::{CreateService, ServiceStatus};

#[allow(clippy::enum_variant_names)]
pub enum Command {
    CreateService {
        name: String,
        service: CreateService,
    },
    StopService {
        name: String,
    },
    ShowService {
        name: String,
    },
}

pub fn bind_api_socket(
    path: impl AsRef<std::path::Path>,
    tx_event: mpsc::Sender<Event>,
) -> io::Result<()> {
    let socket = UnixListener::bind(path)?;

    let router = Router::new()
        .route("/service/{name}", post(create_service))
        .route("/service/{name}/stop", post(stop_service))
        // .route("/service/{name}/start", post(start_service))
        .route("/service/{name}/show", post(show_service))
        .with_state(tx_event);

    tokio::spawn(async move {
        axum::serve(socket, router).await.unwrap();
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
        .unwrap();
    rx.await.unwrap()
}

async fn stop_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command(Command::StopService { name }, tx))
        .await
        .unwrap();
    rx.await.unwrap()
}

async fn show_service(
    Path(name): Path<String>,
    State(tx_events): State<mpsc::Sender<Event>>,
) -> Response {
    let (tx, rx) = oneshot::channel();
    tx_events
        .send(Event::Command(Command::ShowService { name }, tx))
        .await
        .unwrap();
    rx.await.unwrap()
}

impl From<crate::services::Service> for crate::api::Service {
    fn from(value: crate::services::Service) -> Self {
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
            crate::services::ServiceStatus::Stopping { main_pid } => {
                ServiceStatus::Stopping { main_pid }
            }
            crate::services::ServiceStatus::Failed(exit_status) => {
                ServiceStatus::Failed(exit_status)
            }
        }
    }
}
