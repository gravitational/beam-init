use std::path::PathBuf;

#[derive(clap::Parser)]
pub(crate) struct Cli {
    /// Add variables from files to all services started by users
    #[arg(long, value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    pub(crate) environment_file: Vec<PathBuf>,

    /// Bootstrap command and arguments
    #[arg(trailing_var_arg = true, required = true, num_args = 1.., value_hint = clap::ValueHint::CommandWithArguments)]
    pub(crate) command: Vec<String>,
}
