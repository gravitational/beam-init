use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use beam_init_api::{CreateService, Service, ServiceStatus};
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Error;

/// A blocking client for the beam-init API.
///
/// # Examples
///
/// ```no_run
/// use beam_init_api::CreateService;
/// use beam_init_client::blocking::Client;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// client.create_service(
///     "example",
///     CreateService {
///         cmd: "/usr/bin/sleep".to_owned(),
///         args: vec!["infinity".to_owned()],
///         liveness: None,
///         pty: false,
///     },
/// )?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Client {
    client: reqwest::blocking::Client,
}

impl Client {
    /// Creates a client connected to the default beam-init API socket.
    pub fn new() -> Result<Self, Error> {
        Client::with_socket(beam_init_api::API_SOCKET_PATH)
    }

    /// Creates a client connected to the beam-init API at `path`.
    pub fn with_socket(path: impl AsRef<Path>) -> Result<Self, Error> {
        if !std::fs::exists(&path).map_err(Error::Io)? {
            return Err(Error::SocketNotFound);
        }
        let client = reqwest::blocking::ClientBuilder::new()
            .unix_socket(path.as_ref())
            .build()
            .map_err(|e| Error::Creation(e.to_string()))?;

        Ok(Self { client })
    }

    /// Creates and starts a service with the provided configuration.
    ///
    /// Returns the configuration accepted by beam-init.
    pub fn create_service(
        &self,
        name: &str,
        service: CreateService,
    ) -> Result<CreateService, Error> {
        self.post(&service_path(name), service)
    }

    /// Returns the current status of every registered service, keyed by name.
    pub fn list_services(&self) -> Result<BTreeMap<String, ServiceStatus>, Error> {
        self.get("/services")
    }

    /// Stops the service. When `prune` is `true`, the service is removed after it stops.
    pub fn stop_service(&self, name: &str, prune: bool) -> Result<(), Error> {
        if prune {
            self.delete(&service_path(name))
        } else {
            self.post(&service_action_path(name, "stop"), name)
        }
    }

    /// Stops and starts the service.
    pub fn restart_service(&self, name: &str) -> Result<(), Error> {
        self.post(&service_action_path(name, "restart"), name)
    }

    /// Returns the configuration and current state of the service.
    pub fn show_service(&self, name: &str) -> Result<Service, Error> {
        self.post(&service_action_path(name, "show"), name)
    }

    /// Pauses the process for the service.
    pub fn freeze_service(&self, name: &str) -> Result<(), Error> {
        self.post(&service_action_path(name, "freeze"), name)
    }

    /// Resumes the paused process for the service.
    pub fn thaw_service(&self, name: &str) -> Result<(), Error> {
        self.post(&service_action_path(name, "thaw"), name)
    }

    /// Returns the buffered logs for the service.
    pub fn logs(&self, name: &str) -> Result<String, Error> {
        let path = format!("{}?follow=false", service_action_path(name, "logs"));
        let resp: reqwest::blocking::Response = self.get_raw(&path)?;
        resp.text().map_err(|e| Error::Decode(e.to_string()))
    }

    /// Opens a stream of logs for the service.
    ///
    /// The returned reader yields buffered log lines first, then waits for new
    /// output from the service.
    pub fn follow_logs(&self, name: &str) -> Result<impl Read, Error> {
        let path = format!("{}?follow=true", service_action_path(name, "logs"));
        self.get_raw(&path)
    }

    fn request(&self, method: Method, path: &str) -> reqwest::blocking::RequestBuilder {
        debug_assert!(path.starts_with('/'));
        self.client
            .request(method, format!("http://beam-init{path}"))
    }

    fn send(req: reqwest::blocking::RequestBuilder) -> Result<reqwest::blocking::Response, Error> {
        let resp = req
            .send()
            .map_err(|error| Error::Transport(error.to_string()))?;

        if let Err(e) = resp.error_for_status_ref() {
            let body = resp.text().unwrap_or_else(|e| e.to_string());

            let status = e.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

            return Err(Error::Response { status, body });
        }

        Ok(resp)
    }

    fn get_raw(&self, path: &str) -> Result<reqwest::blocking::Response, Error> {
        Self::send(self.request(Method::GET, path))
    }

    fn get<U: DeserializeOwned>(&self, path: &str) -> Result<U, Error> {
        self.get_raw(path)?
            .json()
            .map_err(|e| Error::Decode(e.to_string()))
    }

    fn post<T: Serialize, U: DeserializeOwned>(&self, path: &str, body: T) -> Result<U, Error> {
        Self::send(self.request(Method::POST, path).json(&body))?
            .json()
            .map_err(|e| Error::Decode(e.to_string()))
    }

    fn delete<U: DeserializeOwned>(&self, path: &str) -> Result<U, Error> {
        Self::send(self.request(Method::DELETE, path))?
            .json()
            .map_err(|e| Error::Decode(e.to_string()))
    }
}

fn service_action_path(name: &str, action: &str) -> String {
    format!("{}/{action}", service_path(name))
}

fn service_path(name: &str) -> String {
    format!("/service/{name}")
}
