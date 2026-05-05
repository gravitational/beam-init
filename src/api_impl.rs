use std::io;

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot};

use crate::Event;
use beam_init::api::{CreateService, ServiceStatus};

pub enum Command {
    CreateService {
        name: String,
        service: CreateService,
    },
    StopService {
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
