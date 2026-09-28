#[derive(Debug)]
pub enum DomainError {
    Invalid(&'static str),
    Conflict(&'static str),
    Storage,
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(m) | Self::Conflict(m) => write!(f, "{m}"),
            Self::Storage => write!(f, "storage"),
        }
    }
}

impl std::error::Error for DomainError {}
