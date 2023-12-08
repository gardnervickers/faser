use std::io;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub(crate) struct SubmitError {
    kind: SubmitErrorKind,
}

impl SubmitError {
    pub(crate) fn shutting_down() -> Self {
        Self {
            kind: SubmitErrorKind::ShuttingDown,
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum SubmitErrorKind {
    #[error("reactor is shutting down")]
    ShuttingDown,
}

impl From<SubmitError> for io::Error {
    fn from(value: SubmitError) -> Self {
        match value.kind {
            SubmitErrorKind::ShuttingDown => io::Error::new(io::ErrorKind::Other, value),
        }
    }
}
