use clap::Parser;
use serde::{Deserialize, Serialize};

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

    let client = reqwest::blocking::ClientBuilder::new()
        .unix_socket("/run/beam-init")
        .build()
        .unwrap();

    match args {
        Cli::Start(start) => {
            let resp = client
                .post(format!("http://beam-init/service/{}", start.service))
                .json(&CreateService {
                    cmd: start.command[0].clone(),
                    args: start.command[1..].to_owned(),
                })
                .send()
                .unwrap();
            assert!(resp.status().is_success(), "{resp:?}");
        }
    }
}
