use std::path::PathBuf;

#[derive(clap::Parser)]
pub(crate) struct Cli {
    /// Add variables from files to all services started by users
    #[arg(long, value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    pub(crate) environment_file: Vec<PathBuf>,

    /// The amount of time, measured in seconds, the bootstrap service has to shutdown after a SIGTERM before
    /// it will be sent a SIGKILL. Defaults to 300s when unset.
    #[arg(long, default_value_t = 300)]
    pub(crate) bootstrap_graceful_termination_period: u32,

    /// Bootstrap command and arguments
    #[arg(trailing_var_arg = true, required = true, num_args = 1.., value_hint = clap::ValueHint::CommandWithArguments)]
    pub(crate) command: Vec<String>,
}
