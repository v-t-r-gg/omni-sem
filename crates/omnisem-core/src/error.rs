//! Shared application error types for configuration, indexing, and CLI mapping.

use std::path::PathBuf;

use crate::domain::DomainError;
use crate::storage::StorageError;

/// Process exit categories reserved by the product contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    Success = 0,
    InvalidInput = 2,
    Configuration = 3,
    Filesystem = 4,
    Database = 5,
    PartialIndexing = 6,
    Protocol = 7,
    Internal = 70,
}

impl ExitCode {
    /// Returns the numeric exit status.
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// Configuration and path-boundary failures.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("platform application directories are unavailable")]
    PathsUnavailable,
    #[error("home directory is unavailable for path expansion")]
    HomeUnavailable,
    #[error("configuration file not found: {0}")]
    Missing(PathBuf),
    #[error("invalid configuration at {path}: {message}")]
    Invalid { path: PathBuf, message: String },
    #[error("unknown configuration field: {0}")]
    UnknownField(String),
    #[error("duplicate root path: {0}")]
    DuplicateRootPath(PathBuf),
    #[error("duplicate root name: {0}")]
    DuplicateRootName(String),
    #[error("root not found: {0}")]
    RootNotFound(String),
    #[error("path is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("path does not exist: {0}")]
    MissingPath(PathBuf),
    #[error("I/O error for {path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("BUDGET_PRESET_NOT_FOUND: {0}")]
    BudgetPresetNotFound(String),
}

/// Stable file-read failures.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReadError {
    #[error("FILE_CHANGED_DURING_READ")]
    ChangedDuringRead,
    #[error("file exceeds configured size limit ({size_bytes} bytes)")]
    Oversized { size_bytes: u64 },
    #[error("path is not a regular file: {0}")]
    NotRegularFile(PathBuf),
    #[error("I/O error for {path}: {message}")]
    Io { path: PathBuf, message: String },
}

/// Indexing and scan failures.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Read(#[from] ReadError),
    #[error("discovery failed for root {root_id}: {message}")]
    Discovery { root_id: String, message: String },
    #[error("no enabled roots are configured")]
    NoRoots,
    #[error("internal indexing error: {0}")]
    Internal(String),
}

impl IndexError {
    /// Maps an indexing error to a process exit category.
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Config(_) => ExitCode::Configuration,
            Self::Storage(_) => ExitCode::Database,
            Self::Domain(_) | Self::NoRoots => ExitCode::InvalidInput,
            Self::Read(_) | Self::Discovery { .. } => ExitCode::Filesystem,
            Self::Internal(_) => ExitCode::Internal,
        }
    }
}

/// Retrieval and evaluation failures.
#[derive(Debug, thiserror::Error)]
pub enum RetrievalError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("evaluation bundle error: {0}")]
    Evaluation(String),
    #[error("{code}: {message}")]
    Semantic { code: &'static str, message: String },
    #[error("internal retrieval error: {0}")]
    Internal(String),
}

impl RetrievalError {
    /// Maps a retrieval error to a process exit category.
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Config(error) => error.exit_code(),
            Self::Storage(_) => ExitCode::Database,
            Self::Domain(DomainError::BudgetPresetNotFound(_)) => ExitCode::Configuration,
            Self::Domain(_) | Self::Evaluation(_) => ExitCode::InvalidInput,
            Self::Semantic { .. } => ExitCode::Protocol,
            Self::Internal(_) => ExitCode::Internal,
        }
    }
}

impl ConfigError {
    /// Maps a configuration error to a process exit category.
    #[must_use]
    pub const fn exit_code(&self) -> ExitCode {
        match self {
            Self::Missing(_)
            | Self::Invalid { .. }
            | Self::UnknownField(_)
            | Self::DuplicateRootPath(_)
            | Self::DuplicateRootName(_)
            | Self::RootNotFound(_)
            | Self::NotDirectory(_)
            | Self::MissingPath(_)
            | Self::BudgetPresetNotFound(_) => ExitCode::Configuration,
            Self::PathsUnavailable | Self::HomeUnavailable | Self::Io { .. } => {
                ExitCode::Filesystem
            }
        }
    }
}
