use std::env;

use serde::{Deserialize, Serialize};

// FIXME dedup
#[derive(Serialize, Deserialize)]
pub struct CreateService {
    pub cmd: String,
    pub args: Vec<String>,
}

fn main() {
    let client = reqwest::blocking::ClientBuilder::new()
        .unix_socket("/run/beam-init")
        .build()
        .unwrap();

    let mut args = env::args();
    args.next();
    assert_eq!(args.next().unwrap(), "start");
    let service = args.next().unwrap();
    args.next().unwrap(); // skip --
    let cmd = args.next().unwrap();

    let resp = client
        .post(format!("http://beam-init/service/{service}"))
        .json(&CreateService {
            cmd,
            args: args.collect(),
        })
        .send()
        .unwrap();
    assert!(resp.status().is_success(), "{resp:?}");
}
