use std::io;

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use tokio::net::UnixListener;
use tokio::sync::mpsc;

use crate::Event;

pub enum Command {
    CreateService(String, CreateService),
}

#[derive(Deserialize)]
pub struct CreateService {
    pub cmd: String,
    pub args: Vec<String>,
}

pub fn bind_api_socket(
    path: impl AsRef<std::path::Path>,
    tx_event: mpsc::Sender<Event>,
) -> io::Result<()> {
    let socket = UnixListener::bind(path)?;

    let router = Router::new()
        .route("/service/{name}", post(create_service))
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
) -> Json<()> {
    tx_events
        .send(Event::Command(Command::CreateService(name, service)))
        .await
        .unwrap();
    Json(())
}
