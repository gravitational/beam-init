use std::collections::BTreeMap;
use std::time::Duration;
use std::{fs, process};

use clap::Parser;
use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;

use beam_init::api::{self, Probe};

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

    fn send(req: reqwest::blocking::RequestBuilder) -> Result<reqwest::blocking::Response, Error> {
        let resp = req
            .send()
            .map_err(|error| Error::Internal { error, body: None })?;

        if let Err(error) = resp.error_for_status_ref() {
            let body = resp.text().unwrap_or_else(|err| err.to_string());

            if let Some(status) = error.status()
                && status.is_client_error()
            {
                return Err(Error::User(body));
            }

            return Err(Error::Internal {
                error,
                body: Some(body),
            });
        }

        Ok(resp)
    }

    fn get_raw(&self, path: &str) -> Result<reqwest::blocking::Response, Error> {
        Self::send(self.request(Method::GET, path))
    }

    fn get<U: DeserializeOwned>(&self, path: &str) -> Result<U, Error> {
        self.get_raw(path)?
            .json()
            .map_err(|error| Error::Internal { error, body: None })
    }

    fn post<T: Serialize, U: DeserializeOwned>(&self, path: &str, body: T) -> Result<U, Error> {
        Self::send(self.request(Method::POST, path).json(&body))?
            .json()
            .map_err(|error| Error::Internal { error, body: None })
    }
}

enum Error {
    User(String),
    Internal {
        error: reqwest::Error,
        body: Option<String>,
    },
}

fn show_error_and_exit<T>(err: Error) -> T {
    match err {
        Error::User(err) => eprintln!("{err}"),
        Error::Internal { error, body } => {
            let path = error.url().map_or_else(|| "", |url| url.path()).to_owned();
            if let Some(body) = body {
                eprintln!("{} for {path} with body:\n{body}", error.without_url())
            } else {
                eprintln!("{} for {path}", error.without_url())
            }
        }
    }

    process::exit(1);
}

#[derive(clap::Parser)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

/// Service manager client for beams
#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Create and start a service
    Start {
        /// Name of the service to create
        #[arg(long)]
        name: Option<String>,
        #[arg(trailing_var_arg = true, index = 1, required = true, num_args = 1.., value_hint = clap::ValueHint::CommandWithArguments)]
        command: Vec<String>,
        #[command(flatten)]
        liveness: Option<LivenessProbe>,
    },
    /// Stop a service
    Stop {
        #[arg(index = 1)]
        name: String,
    },
    /// Stop a service if currently running and start it again.
    Restart {
        #[arg(index = 1)]
        name: String,
    },
    /// Freeze all processes of a service
    Freeze {
        #[arg(index = 1)]
        name: String,
    },
    /// Resume all processes of a service
    Thaw {
        #[arg(index = 1)]
        name: String,
    },
    /// Show information about a service
    Show {
        #[arg(index = 1)]
        name: String,
    },
    /// List all services
    List,
    /// Show logs of a service
    Logs {
        #[arg(index = 1)]
        name: String,
        /// Follow logs as they are produced. If not enabled a snapshot of the logs will be shown.
        #[arg(long)]
        follow: bool,
    },
}

// Defaults are from https://github.com/kubernetes/kubernetes/blob/master/pkg/apis/core/v1/defaults.go.
//
// The fields are optional, and only when the port is specified are the other fields accepted.
#[derive(Debug, Clone, clap::Args)]
struct LivenessProbe {
    /// Port the liveness probe connects to.
    #[arg(long = "liveness-port", required = false)]
    port: u16,

    #[arg(long = "liveness-path", default_value = "/readyz", requires = "port")]
    path: String,

    #[arg(long = "liveness-initial-delay-seconds", value_parser = parse_duration_seconds, default_value = "0", requires = "port")]
    initial_delay: Duration,

    #[arg(long = "liveness-period-seconds", value_parser = parse_duration_seconds, default_value = "10", requires = "port")]
    period: Duration,

    #[arg(
        long = "liveness-failure-threshold",
        default_value_t = 3,
        requires = "port"
    )]
    failure_threshold: usize,
}

impl From<LivenessProbe> for Probe {
    fn from(value: LivenessProbe) -> Self {
        Probe {
            port: value.port,
            path: value.path,
            initial_delay: value.initial_delay,
            period: value.period,
            failure_threshold: value.failure_threshold,
        }
    }
}

fn parse_duration_seconds(s: &str) -> Result<Duration, std::num::ParseIntError> {
    Ok(Duration::from_secs(s.parse()?))
}

fn main() {
    let args = Cli::parse();

    let client = Client::new_local();

    match args.command {
        Command::Start {
            name,
            command,
            liveness,
        } => {
            let name = name.unwrap_or_else(gen_name);
            let _resp: api::CreateService = client
                .post(
                    &format!("/service/{}", name),
                    api::CreateService {
                        cmd: command[0].clone(),
                        args: command[1..].to_owned(),
                        liveness: liveness.map(Into::into),
                    },
                )
                .unwrap_or_else(show_error_and_exit);
            eprintln!("Started service {name}");
        }
        Command::Stop { name } => {
            let _resp: () = client
                .post(&format!("/service/{}/stop", name), name)
                .unwrap_or_else(show_error_and_exit);
        }
        Command::Restart { name } => {
            let _resp: () = client
                .post(&format!("/service/{}/restart", name), name)
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

fn gen_name() -> String {
    let mut buf = [0u8; 8];
    // SAFETY: We pass a valid mutable byte array of the given size.
    unsafe { libc::getrandom(buf.as_mut_ptr().cast(), buf.len(), 0) };
    format!("{:016x}", u64::from_ne_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_config_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    mod liveness {
        use super::*;

        fn parse(args: &[&str]) -> Option<LivenessProbe> {
            let argv = [&["beamctl", "start"], args, &["--", "sleep", "10"]].concat();
            match Cli::try_parse_from(argv).expect("should parse").command {
                Command::Start { liveness, .. } => liveness,
                other => panic!("expected a Start command, got {other:?}"),
            }
        }

        #[test]
        fn no_flags() {
            assert!(parse(&[]).is_none());
        }

        #[test]
        fn port_enables_probe_with_defaults() {
            let probe = parse(&["--liveness-port", "8080"]).unwrap();
            assert_eq!(probe.port, 8080);

            // The defaults.
            assert_eq!(probe.path, "/readyz");
            assert_eq!(probe.initial_delay, Duration::from_secs(0));
            assert_eq!(probe.period, Duration::from_secs(10));
            assert_eq!(probe.failure_threshold, 3);
        }

        #[test]
        fn flags_without_port_are_rejected() {
            // The other liveness flags should only parse when a port has been specified.
            let flags = [
                vec!["--liveness-path", "/x"],
                vec!["--liveness-initial-delay-seconds", "5"],
                vec!["--liveness-period-seconds", "2"],
                vec!["--liveness-failure-threshold", "1"],
            ];

            for flag in flags {
                let argv = [
                    &["beamctl", "start"],
                    flag.as_slice(),
                    &["--", "sleep", "10"],
                ]
                .concat();
                assert!(
                    Cli::try_parse_from(argv).is_err(),
                    "{flag:?} without `--liveness-port` should be rejected",
                );
            }
        }
    }
}
