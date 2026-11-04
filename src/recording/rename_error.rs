use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    #[error("a recording needs a name")]
    EmptyTitle,
    #[error("names cannot contain '/' or start with '.'")]
    InvalidTitle,
    #[error("{} already exists", .0.display())]
    AlreadyExists(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
}
