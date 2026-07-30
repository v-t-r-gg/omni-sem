//! Platform-aware application directories.

use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::error::ConfigError;

const QUALIFIER: &str = "dev";
const ORGANIZATION: &str = "OmniSem";
const APPLICATION: &str = "omnisem";

/// Resolved Omni-Sem filesystem locations for one installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub default_database_path: PathBuf,
    pub log_dir: PathBuf,
}

impl AppPaths {
    /// Resolves platform-native configuration and data directories.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::PathsUnavailable`] when the host has no usable
    /// user project directories.
    pub fn discover() -> Result<Self, ConfigError> {
        let dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .ok_or(ConfigError::PathsUnavailable)?;
        Ok(Self::from_project_dirs(&dirs))
    }

    /// Builds paths under an explicit base directory (tests and overrides).
    #[must_use]
    pub fn for_base(base: impl AsRef<Path>) -> Self {
        let base = base.as_ref();
        let config_dir = base.join("config");
        let data_dir = base.join("data");
        Self {
            config_file: config_dir.join("config.toml"),
            default_database_path: data_dir.join("index.sqlite3"),
            log_dir: data_dir.join("logs"),
            config_dir,
            data_dir,
        }
    }

    fn from_project_dirs(dirs: &ProjectDirs) -> Self {
        let config_dir = dirs.config_dir().to_path_buf();
        let data_dir = dirs.data_dir().to_path_buf();
        Self {
            config_file: config_dir.join("config.toml"),
            default_database_path: data_dir.join("index.sqlite3"),
            log_dir: data_dir.join("logs"),
            config_dir,
            data_dir,
        }
    }

    /// Creates configuration and data directories with restrictive permissions.
    ///
    /// # Errors
    ///
    /// Returns filesystem errors when directories cannot be created.
    pub fn ensure_layout(&self) -> Result<(), ConfigError> {
        create_private_dir(&self.config_dir)?;
        create_private_dir(&self.data_dir)?;
        create_private_dir(&self.log_dir)?;
        Ok(())
    }
}

/// Creates a directory and, on Unix, restricts it to the current user.
///
/// # Errors
///
/// Returns [`ConfigError::Io`] when creation or permission updates fail.
pub fn create_private_dir(path: &Path) -> Result<(), ConfigError> {
    fs::create_dir_all(path).map_err(|error| ConfigError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    restrict_permissions(path)?;
    Ok(())
}

/// Restricts a file or directory to owner read/write (and execute for dirs).
///
/// # Errors
///
/// Returns [`ConfigError::Io`] when permissions cannot be updated.
pub fn restrict_permissions(path: &Path) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        let permissions = fs::Permissions::from_mode(mode);
        fs::set_permissions(path, permissions).map_err(|error| ConfigError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Expands a leading `~/` using the process home directory once.
///
/// # Errors
///
/// Returns [`ConfigError::HomeUnavailable`] when home cannot be resolved.
pub fn expand_user_path(input: &str) -> Result<PathBuf, ConfigError> {
    if let Some(rest) = input.strip_prefix("~/") {
        Ok(home_dir()?.join(rest))
    } else if input == "~" {
        home_dir()
    } else {
        Ok(PathBuf::from(input))
    }
}

fn home_dir() -> Result<PathBuf, ConfigError> {
    if let Some(dirs) = directories::UserDirs::new() {
        return Ok(dirs.home_dir().to_path_buf());
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(ConfigError::HomeUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn for_base_layout_is_private_on_unix() {
        let temp = TempDir::new().unwrap();
        let paths = AppPaths::for_base(temp.path());
        paths.ensure_layout().unwrap();
        assert!(paths.config_dir.is_dir());
        assert!(paths.data_dir.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&paths.config_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        }
    }

    #[test]
    fn expand_user_path_preserves_absolute() {
        assert_eq!(
            expand_user_path("/tmp/notes").unwrap(),
            PathBuf::from("/tmp/notes")
        );
    }
}
