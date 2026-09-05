#[derive(Debug)]
pub(crate) enum MutationRunnerError {
    Resolution(String),
    Integrity(String),
}

impl MutationRunnerError {
    pub(crate) fn resolution(error: impl std::fmt::Display) -> Self {
        Self::Resolution(error.to_string())
    }

    pub(crate) fn integrity(message: impl Into<String>) -> Self {
        Self::Integrity(message.into())
    }

    pub(crate) fn source_intact(&self) -> bool {
        matches!(self, Self::Resolution(_))
    }
}

impl std::fmt::Display for MutationRunnerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resolution(message) | Self::Integrity(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for MutationRunnerError {}
