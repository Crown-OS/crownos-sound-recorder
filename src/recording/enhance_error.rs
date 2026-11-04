use std::io;

#[derive(Debug, thiserror::Error)]
pub enum EnhanceError {
    #[error("the voice service is not available: {0}")]
    ServiceUnavailable(#[source] crownos_ipc::Error),
    #[error("the voice service failed: {0}")]
    Service(#[from] crownos_ipc::Error),
    #[error("enhancement failed: {0}")]
    JobFailed(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Media(#[from] anyhow::Error),
    #[error("the enhancement thread panicked")]
    Panicked,
}
