use std::process;
use std::time::Duration;

use clap::{CommandFactory, Parser};
use clap_complete::{Shell, generate};

use beam_init_api::Probe;
use beam_init_client::blocking::Client;

const VERSION: &str = match option_env!("BEAM_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
const GIT_SHA: &str = match option_env!("GIT_SHA") {
    Some(sha) => sha,
    None => "unknown",
};

#[cfg(feature = "unstable-pty")]
mod terminal;

fn show_error_and_exit<T>(err: beam_init_client::Error) -> T {
    match err {
        beam_init_client::Error::Response { status, body } if status.is_client_error() => {
            eprintln!("{body}")
        }
        beam_init_client::Error::SocketNotFound => eprintln!(
            "{err}\nhint: beamctl only works inside containers that use beam-init as init process"
        ),
        _ => eprintln!("{err}"),
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
        /// Tag to associate this service with
        #[arg(long)]
        labels: Option<String>,
        #[arg(long)]
        #[cfg(feature = "unstable-pty")]
        pty: bool,
        #[arg(trailing_var_arg = true, index = 1, required = true, num_args = 1.., value_hint = clap::ValueHint::CommandWithArguments)]
        command: Vec<String>,
        #[command(flatten)]
        liveness: Option<LivenessProbe>,
    },
    /// Stop a service
    Stop {
        #[arg(index = 1)]
        name: String,
        /// Lookup service by selector
        #[arg(long)]
        selector: bool,
        /// Remove this service from the list of services.
        #[arg(long)]
        prune: bool,
    },
    /// Stop a service if currently running and start it again.
    Restart {
        #[arg(index = 1)]
        name: String,
        /// Lookup service by selector
        #[arg(long)]
        selector: bool,
    },
    /// Freeze all processes of a service
    Freeze {
        #[arg(index = 1)]
        name: String,
        /// Lookup service by selector
        #[arg(long)]
        selector: bool,
    },
    /// Resume all processes of a service
    Thaw {
        #[arg(index = 1)]
        name: String,
        /// Lookup service by selector
        #[arg(long)]
        selector: bool,
    },
    /// Show information about a service
    Show {
        #[arg(index = 1)]
        name: String,
        /// Lookup service by selector
        #[arg(long)]
        selector: bool,
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
    /// Attach to the PTY of a running service
    #[cfg(feature = "unstable-pty")]
    Attach {
        #[arg(index = 1)]
        name: String,
    },
    /// Generate command-line completions for the given shell.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },
    /// Show the version of beamctl and beam-init
    Version,
}

// Defaults are from https://github.com/kubernetes/kubernetes/blob/master/pkg/apis/core/v1/defaults.go.
//
// The fields are optional, and only when the port is specified are the other fields accepted.
#[derive(Debug, Clone, clap::Args)]
struct LivenessProbe {
    /// Port the liveness probe connects to.
    #[arg(long = "liveness-port", required = false)]
    port: u16,

    #[arg(long = "liveness-path", default_value = "/livez", requires = "port")]
    path: String,

    #[arg(
        long = "liveness-max-retries",
        default_value = "1024",
        requires = "port"
    )]
    max_retries: usize,

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
        let LivenessProbe {
            port,
            path,
            max_retries,
            initial_delay,
            period,
            failure_threshold,
        } = value;

        Probe {
            port,
            path,
            max_retries,
            initial_delay,
            period,
            failure_threshold,
        }
    }
}

fn parse_duration_seconds(s: &str) -> Result<Duration, std::num::ParseIntError> {
    Ok(Duration::from_secs(s.parse()?))
}

fn main() {
    let args = Cli::parse();

    let client = Client::new().unwrap_or_else(show_error_and_exit);

    match args.command {
        Command::Start {
            name,
            labels,
            command,
            liveness,
            #[cfg(feature = "unstable-pty")]
            pty,
        } => {
            #[cfg(not(feature = "unstable-pty"))]
            let pty = false;
            let name = name.unwrap_or_else(gen_name);
            //FIXME
            let labels = labels.into_iter().map(|x| ("tag".to_string(), x)).collect();
            let _resp = client
                .create_service(
                    &name,
                    beam_init_api::CreateService {
                        cmd: command[0].clone(),
                        args: command[1..].to_owned(),
                        liveness: liveness.map(Into::into),
                        pty,
                        labels,
                    },
                )
                .unwrap_or_else(show_error_and_exit);
            eprintln!("Started service {name}");

            #[cfg(feature = "unstable-pty")]
            if pty {
                attach(client, name);
            }
        }
        Command::Stop {
            name,
            selector,
            prune,
        } => {
            for name in service_match(&client, name, selector) {
                client
                    .stop_service(&name, prune)
                    .unwrap_or_else(show_error_and_exit)
            }
        }
        Command::Restart { name, selector } => {
            for name in service_match(&client, name, selector) {
                let _resp: () = client
                    .restart_service(&name)
                    .unwrap_or_else(show_error_and_exit);
            }
        }
        Command::Freeze { name, selector } => {
            for name in service_match(&client, name, selector) {
                let _resp: () = client
                    .freeze_service(&name)
                    .unwrap_or_else(show_error_and_exit);
            }
        }
        Command::Thaw { name, selector } => {
            for name in service_match(&client, name, selector) {
                let _resp: () = client
                    .thaw_service(&name)
                    .unwrap_or_else(show_error_and_exit);
            }
        }
        Command::Logs { name, follow } => {
            let name = prefix_match(&client, name);
            if follow {
                let mut resp = client
                    .follow_logs(&name)
                    .unwrap_or_else(show_error_and_exit);
                std::io::copy(&mut resp, &mut std::io::stdout()).unwrap();
            } else {
                let logs = client.logs(&name).unwrap_or_else(show_error_and_exit);
                print!("{logs}");
            }
        }
        Command::Show { name, selector } => {
            for name in service_match(&client, name, selector) {
                let service = client
                    .show_service(&name)
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
        }
        Command::List => {
            let services = client.list_services().unwrap_or_else(show_error_and_exit);

            if args.json {
                serde_json::to_writer_pretty(std::io::stdout(), &services).unwrap();
                println!();
            } else {
                for (name, status) in services {
                    println!("{name} ({status})");
                }
            }
        }
        #[cfg(feature = "unstable-pty")]
        Command::Attach { name } => {
            let name = prefix_match(&client, name);
            attach(client, name);
        }
        Command::Completions { shell } => {
            let mut command = Cli::command();

            generate(shell, &mut command, "beamctl", &mut std::io::stdout());
        }
        Command::Version => {
            println!("beamctl: Version: {} - SHA: {}", VERSION, GIT_SHA);
            let resp = client.version().unwrap_or_else(show_error_and_exit);
            println!("beam-init: Version: {} - SHA: {}", resp.version, resp.sha);
        }
    }
}

/// Attach to the given service
#[cfg(feature = "unstable-pty")]
fn attach(client: Client, name: String) {
    let service = client
        .show_service(&name)
        .unwrap_or_else(show_error_and_exit);

    let (pid, pty) = match service.status {
        beam_init_api::ServiceStatus::Running {
            ref pty, main_pid, ..
        }
        | beam_init_api::ServiceStatus::Frozen {
            ref pty, main_pid, ..
        } => {
            if let Some((index, _)) = pty {
                if let Some(fd) = get_fd_from_store(*index) {
                    (main_pid, fd)
                } else {
                    // We raced with the process exiting
                    let service = client
                        .show_service(&name)
                        .unwrap_or_else(show_error_and_exit);
                    println!("could not attach to {name} ({})", service.status);
                    return;
                }
            } else {
                println!("service {name} does not have a pty attached");
                return;
            }
        }
        _ => {
            println!("could not attach to {name} ({})", service.status);
            return;
        }
    };

    // we must get rid of the client before entering the terminal, because
    // it interferes with signal handling
    drop(client);

    if let Err(err) = terminal::manage(pid, pty) {
        println!("pty error for service {name} ({})", err);
    } else {
        // Retrieve the new status, which could have changed.
        let client = Client::new().unwrap_or_else(show_error_and_exit);
        let service = client
            .show_service(&name)
            .unwrap_or_else(show_error_and_exit);

        println!("detached from {name} ({})", service.status);
        if matches!(service.status, beam_init_api::ServiceStatus::Running { .. }) {
            println!("to reattach use `beamctl attach {name}`");
        }
    }
}

/// Retrieve a file descriptor over the dedicated socket
#[cfg(feature = "unstable-pty")]
fn get_fd_from_store(fdstore_idx: u64) -> Option<std::os::fd::OwnedFd> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    use beam_init::system::unix_socket::socket_recv_fd;

    let mut socket = UnixStream::connect(beam_init_api::FD_SOCKET_PATH).unwrap();
    socket.write_all(&u64::to_le_bytes(fdstore_idx)).unwrap();
    let (_len, fd) = socket_recv_fd(&socket, &mut [0]).unwrap();
    fd
}

/// Match services based on a prefix of the name or based on a tag selection
fn service_match(
    client: &Client,
    name: String,
    match_on_tag: bool,
) -> Box<dyn Iterator<Item = String>> {
    if match_on_tag {
        use beam_init_api::ServiceStatus;
        let services = client.list_services().unwrap_or_else(show_error_and_exit);

        let results =
            services
                .into_iter()
                .filter_map(move |(service_name, status)| {
                    if let ServiceStatus::Running { labels, .. }
                    | ServiceStatus::Frozen { labels, .. } = status
                    //FIXME
                        && labels.get("tag").is_some_and(|tag| tag == &name)
                    {
                        Some(service_name)
                    } else {
                        None
                    }
                });

        Box::new(results)
    } else {
        let name = prefix_match(client, name);
        Box::new(std::iter::once(name))
    }
}

/// As a userfriendliness feature, allow the user to match a service by only
/// matching a prefix instead of the full service name.
fn prefix_match(client: &Client, name: String) -> String {
    let mut services = client.list_services().unwrap_or_else(show_error_and_exit);

    let mut service_names = services
        .split_off(&name)
        .into_keys()
        .take_while(|key| key.starts_with(&name));

    if let Some(found_name) = service_names.next()
        && let None = service_names.next()
        && found_name != "bootstrap"
    {
        // the prefix uniquely defines exactly one service
        found_name
    } else {
        name
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
            assert_eq!(probe.path, "/livez");
            assert_eq!(probe.initial_delay, Duration::from_secs(0));
            assert_eq!(probe.period, Duration::from_secs(10));
            assert_eq!(probe.failure_threshold, 3);
            assert_eq!(probe.max_retries, 1024);
        }

        #[test]
        fn flags_without_port_are_rejected() {
            // The other liveness flags should only parse when a port has been specified.
            let flags = [
                vec!["--liveness-path", "/x"],
                vec!["--liveness-initial-delay-seconds", "5"],
                vec!["--liveness-period-seconds", "2"],
                vec!["--liveness-failure-threshold", "1"],
                vec!["--liveness-max-retries", "32"],
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
