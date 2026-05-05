use clap::Parser;
use http_body_util::BodyExt;
use hyper::Request;
use hyper::client::conn::http1;
use serde::Serialize;
use serde::de::DeserializeOwned;

use beam_init::api;
use tokio::net::UnixStream;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, Error>;

async fn post<T: Serialize, U: DeserializeOwned>(path: &str, body: T) -> Result<U> {
    debug_assert!(path.starts_with('/'));
    let stream = UnixStream::connect("/run/beam-init").await?;
    let io = hyper_util::rt::TokioIo::new(stream);
    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(conn);

    let body = serde_json::to_vec(&body)?;
    let req = Request::builder()
        .method(hyper::Method::POST)
        .uri(path)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(http_body_util::Full::new(hyper::body::Bytes::from(body)))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();
    let bytes = resp.into_body().collect().await?.to_bytes();

    if !status.is_success() {
        return Err(std::io::Error::other(format!(
            "POST {path} failed with {status}: {}",
            String::from_utf8_lossy(&bytes)
        ))
        .into());
    }

    Ok(serde_json::from_slice(&bytes)?)
}

#[derive(clap::Parser)]
enum Cli {
    Start(StartArgs),
    Stop {
        #[arg(index = 1)]
        name: String,
    },
}

#[derive(clap::Args)]
struct StartArgs {
    #[arg(index = 1)]
    service: String,
    #[arg(trailing_var_arg = true, index = 2, required = true, num_args = 1.., value_hint = clap::ValueHint::CommandWithArguments)]
    command: Vec<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = Cli::parse();

    match args {
        Cli::Start(start) => {
            let _resp: api::CreateService = post(
                &format!("/service/{}", start.service),
                api::CreateService {
                    cmd: start.command[0].clone(),
                    args: start.command[1..].to_owned(),
                },
            )
            .await
            .unwrap();
        }
        Cli::Stop { name } => {
            let _resp: () = post(&format!("/service/{}/stop", name), name)
                .await
                .unwrap();
        }
    }
}
