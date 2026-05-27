use std::collections::BTreeMap;
use std::{fs, process};

use clap::Parser;
use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;

use beam_init::api;

struct Client {
    client: reqwest::blocking::Client,
}

impl Client {
    fn new_local() -> Self {
        if !fs::exists(api::SOCKET_PATH).unwrap_or(false) {
            eprintln!("error: {} doesn't exist.", api::SOCKET_PATH);
            eprintln!(
                "hint: beamctl only works inside containers that use beam-init as init process",
            );
            process::exit(1);
        }

        let client = reqwest::blocking::ClientBuilder::new()
            .unix_socket(api::SOCKET_PATH)
            .build()
            .unwrap_or_else(|err| {
                eprintln!("Failed to initialize HTTP client: {err}");
                process::exit(1);
            });

        Client { client }
    }

    fn request(&self, method: Method, path: &str) -> reqwest::blocking::RequestBuilder {
        debug_assert!(path.starts_with('/'));
        self.client
            .request(method, format!("http://beam-init{path}"))
    }

    fn send(
        req: reqwest::blocking::RequestBuilder,
    ) -> reqwest::Result<reqwest::blocking::Response> {
        let resp = req.send()?;

        // FIXME add response body to error
        resp.error_for_status_ref()?;

        Ok(resp)
    }

    fn get_raw(&self, path: &str) -> reqwest::Result<reqwest::blocking::Response> {
        Self::send(self.request(Method::GET, path))
    }

    fn get<U: DeserializeOwned>(&self, path: &str) -> reqwest::Result<U> {
        self.get_raw(path)?.json()
    }

    fn post<T: Serialize, U: DeserializeOwned>(&self, path: &str, body: T) -> reqwest::Result<U> {
        Self::send(self.request(Method::POST, path).json(&body))?.json()
    }
}

fn show_error_and_exit<T>(err: reqwest::Error) -> T {
    eprintln!("{err}");
    process::exit(1);
}

#[derive(clap::Parser)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    Start(StartArgs),
    Stop {
        #[arg(index = 1)]
        name: String,
    },
    Freeze {
        #[arg(index = 1)]
        name: String,
    },
    Thaw {
        #[arg(index = 1)]
        name: String,
    },
    Show {
        #[arg(index = 1)]
        name: String,
    },
    List,
    Logs {
        #[arg(index = 1)]
        name: String,
        #[arg(long)]
        follow: bool,
    },
}

#[derive(clap::Args)]
struct StartArgs {
    #[arg(index = 1)]
    service: String,
    #[arg(trailing_var_arg = true, index = 2, required = true, num_args = 1.., value_hint = clap::ValueHint::CommandWithArguments)]
    command: Vec<String>,
}

fn main() {
    let args = Cli::parse();

    let client = Client::new_local();

    match args.command {
        Command::Start(start) => {
            let _resp: api::CreateService = client
                .post(
                    &format!("/service/{}", start.service),
                    api::CreateService {
                        cmd: start.command[0].clone(),
                        args: start.command[1..].to_owned(),
                    },
                )
                .unwrap_or_else(show_error_and_exit);
        }
        Command::Stop { name } => {
            let _resp: () = client
                .post(&format!("/service/{}/stop", name), name)
                .unwrap_or_else(show_error_and_exit);
        }
        Command::Freeze { name } => {
            let _resp: () = client
                .post(&format!("/service/{}/freeze", name), name)
                .unwrap_or_else(show_error_and_exit);
        }
        Command::Thaw { name } => {
            let _resp: () = client
                .post(&format!("/service/{}/thaw", name), name)
                .unwrap_or_else(show_error_and_exit);
        }
        Command::Logs { name, follow } => {
            let mut resp = client
                .get_raw(&format!("/service/{name}/logs?follow={follow}"))
                .unwrap_or_else(show_error_and_exit);
            std::io::copy(&mut resp, &mut std::io::stdout()).unwrap();
        }
        Command::Show { name } => {
            let service: beam_init::api::Service = client
                .post(&format!("/service/{}/show", name), &name)
                .unwrap_or_else(show_error_and_exit);

            if args.json {
                serde_json::to_writer_pretty(std::io::stdout(), &service).unwrap();
                println!();
            } else {
                // Handle formatting if there are no arguments.
                let mut args = service.args;
                args.insert(0, service.cmd);

                println!("{name} ({}): {}", service.status, args.join(" "));
            }
        }
        Command::List => {
            let services: BTreeMap<String, beam_init::api::ServiceStatus> =
                client.get("/services").unwrap_or_else(show_error_and_exit);

            if args.json {
                serde_json::to_writer_pretty(std::io::stdout(), &services).unwrap();
                println!();
            } else {
                for (name, status) in services {
                    println!("{name} ({status})")
                }
            }
        }
    }
}
