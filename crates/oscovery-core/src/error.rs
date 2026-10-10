use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("the index did not parse: {0}")]
    Decode(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The index is newer than this client understands.
    #[error("the index is version {0}; this Oscovery reads up to {1}")]
    IndexVersion(u32, u32),
    #[error("{0}")]
    Message(String),
}

impl Error {
    pub fn message(text: impl Into<String>) -> Self {
        Error::Message(text.into())
    }
}
