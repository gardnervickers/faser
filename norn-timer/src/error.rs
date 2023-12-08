#[derive(thiserror::Error, Debug, Clone, Copy)]
#[error(transparent)]
pub struct Error {
    kind: ErrorKind,
}

impl Error {
    pub(super) fn shutdown() -> Self {
        Self {
            kind: ErrorKind::Shutdown,
        }
    }
}

#[derive(thiserror::Error, Debug, Clone, Copy)]
pub enum ErrorKind {
    #[error("the timer has shut down")]
    Shutdown,
}
