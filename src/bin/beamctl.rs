use std::collections::BTreeMap;

use clap::Parser;
use serde::Serialize;
use serde::de::DeserializeOwned;

use beam_init::api;

struct Client {
    client: reqwest::blocking::Client,
}

impl Client {
    fn new_local() -> reqwest::Result<Self> {
        let client = reqwest::blocking::ClientBuilder::new()
            .unix_socket("/run/beam-init")
            .build()?;

        Ok(Client { client })
    }

    fn get<U: DeserializeOwned>(&self, path: &str) -> reqwest::Result<U> {
        debug_assert!(path.starts_with('/'));

        let resp = self.client.get(format!("http://beam-init{path}")).send()?;

        // FIXME add response body to error
        resp.error_for_status_ref()?;

        resp.json()
    }

    fn post<T: Serialize, U: DeserializeOwned>(&self, path: &str, body: T) -> reqwest::Result<U> {
        debug_assert!(path.starts_with('/'));

        let resp = self
            .client
            .post(format!("http://beam-init{path}"))
            .json(&body)
            .send()?;

        // FIXME add response body to error
        resp.error_for_status_ref()?;

        resp.json()
    }
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

    let client = Client::new_local().unwrap();

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
                .unwrap();
        }
        Command::Stop { name } => {
            let _resp: () = client
                .post(&format!("/service/{}/stop", name), name)
                .unwrap();
        }
        Command::Logs { name, follow } => {
            let mut resp = client
                .client
                .get(format!(
                    "http://beam-init/service/{name}/logs?follow={follow}"
                ))
                .send()
                .unwrap();
            resp.error_for_status_ref().unwrap();
            std::io::copy(&mut resp, &mut std::io::stdout()).unwrap();
        }
        Command::Show { name } => {
            let service: beam_init::api::Service = client
                .post(&format!("/service/{}/show", name), &name)
                .unwrap();

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
                client.get("/services").unwrap();

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
