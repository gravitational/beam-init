use clap::Parser;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

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
enum Cli {
    Start(StartArgs),
}

#[derive(clap::Args)]
struct StartArgs {
    #[arg(index = 1)]
    service: String,
    #[arg(trailing_var_arg = true, index = 2, num_args = 1.., value_hint = clap::ValueHint::CommandWithArguments)]
    command: Vec<String>,
}

// FIXME dedup
#[derive(Serialize, Deserialize)]
pub struct CreateService {
    pub cmd: String,
    pub args: Vec<String>,
}

fn main() {
    let args = Cli::parse();

    let client = Client::new_local().unwrap();

    match args {
        Cli::Start(start) => {
            let _resp: CreateService = client
                .post(
                    &format!("/service/{}", start.service),
                    CreateService {
                        cmd: start.command[0].clone(),
                        args: start.command[1..].to_owned(),
                    },
                )
                .unwrap();
        }
    }
}
