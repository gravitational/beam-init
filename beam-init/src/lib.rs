#![deny(clippy::unwrap_used)]

pub const VERSION: &str = match option_env!("VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
pub const GIT_SHA: &str = match option_env!("GIT_SHA") {
    Some(sha) => sha,
    None => "unknown",
};

pub mod system;
