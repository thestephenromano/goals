use thiserror::Error;

#[derive(Debug, Error)]
pub enum GoalsError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    /// The request breaks a rule of the model; nothing was changed.
    #[error("{0}")]
    Validation(String),
    #[error("not found: {0}")]
    NotFound(String),
    /// A stored value no version of this app writes.
    #[error("stored data is damaged: {0}")]
    Damaged(String),
    #[error("could not determine the user's data directory")]
    NoDataDir,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, GoalsError>;
