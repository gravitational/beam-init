use reqwest::StatusCode;

/// An error returned by a beam-init client operation.
#[derive(Debug)]
pub enum Error {
    /// An I/O operation failed while locating the API socket.
    Io(std::io::Error),

    /// The beam-init API socket does not exist.
    SocketNotFound,

    /// The underlying HTTP client could not be created.
    Creation(String),

    /// A request could not be sent or its response could not be received.
    Transport(String),

    /// A successful response could not be decoded.
    Decode(String),

    /// The server returned an unsuccessful HTTP status.
    Response {
        /// HTTP status returned by the server.
        status: StatusCode,
        /// Response body returned by the server.
        body: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(error) => write!(f, "i/o error: {error}"),
            Error::SocketNotFound => f.write_str("beam-init socket not found"),
            Error::Creation(error) => write!(f, "client creation failed: {error}"),
            Error::Transport(error) => write!(f, "request failed: {error}"),
            Error::Decode(error) => write!(f, "response decoding failed: {error}"),
            Error::Response { status, body } => write!(f, "server returned {status}: {body}"),
        }
    }
}

impl std::error::Error for Error {}
