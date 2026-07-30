//! TOML configuration loading, validation, and persistence.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discovery::default_exclude_patterns;
use crate::domain::{Root, RootId, SensitivityScope, SensitivityTag, Timestamp};
use crate::error::ConfigError;
use crate::hash::blake3_hex;
use crate::paths::{AppPaths, expand_user_path, restrict_permissions};

/// Top-level Omni-Sem configuration document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub general: GeneralConfig,
    #[serde(default)]
    pub roots: Vec<RootConfig>,
}

/// Global settings that are not root-specific.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralConfig {
    pub database_path: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_log_level() -> String {
    "info".into()
}

/// One approved root as stored in configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootConfig {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub sensitivity: Vec<SensitivityConfig>,
    #[serde(default)]
    pub follow_symlinks: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Sensitivity tag as represented in TOML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensitivityConfig {
    pub pattern: String,
    pub scope: String,
}

impl AppConfig {
    /// Builds a safe default configuration for a fresh installation.
    #[must_use]
    pub fn default_for(paths: &AppPaths) -> Self {
        Self {
            general: GeneralConfig {
                database_path: paths.default_database_path.display().to_string(),
                log_level: default_log_level(),
            },
            roots: Vec::new(),
        }
    }

    /// Loads and validates configuration from disk.
    ///
    /// # Errors
    ///
    /// Returns configuration errors for missing files, unknown fields, or invalid values.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = fs::read_to_string(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ConfigError::Missing(path.to_path_buf())
            } else {
                ConfigError::Io {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                }
            }
        })?;
        let config: Self = toml::from_str(&text).map_err(|error| {
            let message = error.to_string();
            if message.contains("unknown field") {
                ConfigError::UnknownField(message)
            } else {
                ConfigError::Invalid {
                    path: path.to_path_buf(),
                    message,
                }
            }
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Serializes configuration to disk with restrictive permissions.
    ///
    /// # Errors
    ///
    /// Returns I/O or validation errors.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| ConfigError::Io {
                path: parent.to_path_buf(),
                message: error.to_string(),
            })?;
        }
        let text = toml::to_string_pretty(self).map_err(|error| ConfigError::Invalid {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        fs::write(path, text).map_err(|error| ConfigError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        restrict_permissions(path)?;
        Ok(())
    }

    /// Validates structural invariants without filesystem access.
    ///
    /// # Errors
    ///
    /// Returns duplicate name/path or identifier errors.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut paths = Vec::new();
        let mut names = Vec::new();
        for root in &self.roots {
            root.id
                .parse::<RootId>()
                .map_err(|_| ConfigError::Invalid {
                    path: PathBuf::from("config"),
                    message: format!("invalid root id '{}'", root.id),
                })?;
            if root.name.trim().is_empty() {
                return Err(ConfigError::Invalid {
                    path: PathBuf::from("config"),
                    message: "root name must not be empty".into(),
                });
            }
            if names.iter().any(|name| name == &root.name) {
                return Err(ConfigError::DuplicateRootName(root.name.clone()));
            }
            names.push(root.name.clone());
            if paths.iter().any(|path| path == &root.path) {
                return Err(ConfigError::DuplicateRootPath(PathBuf::from(&root.path)));
            }
            paths.push(root.path.clone());
            for tag in &root.sensitivity {
                SensitivityScope::from_str_cfg(&tag.scope).map_err(|message| {
                    ConfigError::Invalid {
                        path: PathBuf::from("config"),
                        message,
                    }
                })?;
            }
        }
        Ok(())
    }

    /// Resolves the configured database path with home expansion.
    ///
    /// # Errors
    ///
    /// Returns path expansion failures.
    pub fn database_path(&self) -> Result<PathBuf, ConfigError> {
        expand_user_path(&self.general.database_path)
    }

    /// Converts configuration roots into domain roots.
    ///
    /// # Errors
    ///
    /// Returns conversion failures for invalid IDs or sensitivity scopes.
    pub fn domain_roots(&self) -> Result<Vec<Root>, ConfigError> {
        self.roots.iter().map(RootConfig::to_domain).collect()
    }

    /// Finds a root configuration by opaque ID.
    #[must_use]
    pub fn find_root(&self, root_id: &str) -> Option<&RootConfig> {
        self.roots.iter().find(|root| root.id == root_id)
    }
}

impl RootConfig {
    /// Creates a new approved root configuration with safe defaults.
    #[must_use]
    pub fn new_approved(id: RootId, name: String, canonical_path: &Path) -> Self {
        Self {
            id: id.to_string(),
            name,
            path: canonical_path.display().to_string(),
            include: default_include_patterns(),
            exclude: default_exclude_patterns(),
            sensitivity: Vec::new(),
            follow_symlinks: false,
            enabled: true,
        }
    }

    /// Converts this configuration entry into a domain root.
    ///
    /// # Errors
    ///
    /// Returns configuration errors for invalid identifiers or sensitivity scopes.
    pub fn to_domain(&self) -> Result<Root, ConfigError> {
        let id = self
            .id
            .parse::<RootId>()
            .map_err(|_| ConfigError::Invalid {
                path: PathBuf::from("config"),
                message: format!("invalid root id '{}'", self.id),
            })?;
        let sensitivity_tags = self
            .sensitivity
            .iter()
            .map(|tag| {
                Ok(SensitivityTag {
                    pattern: tag.pattern.clone(),
                    scope: SensitivityScope::from_str_cfg(&tag.scope).map_err(|message| {
                        ConfigError::Invalid {
                            path: PathBuf::from("config"),
                            message,
                        }
                    })?,
                })
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;
        let now = Timestamp::now().unwrap_or_else(|_| Timestamp::from_millis(0));
        let mut root = Root {
            id,
            canonical_path: PathBuf::from(&self.path),
            display_name: self.name.clone(),
            include_patterns: self.include.clone(),
            exclude_patterns: self.exclude.clone(),
            sensitivity_tags,
            follow_symlinks: self.follow_symlinks,
            enabled: self.enabled,
            created_at: now,
            updated_at: now,
            config_fingerprint: String::new(),
        };
        root.config_fingerprint = config_fingerprint(&root);
        Ok(root)
    }
}

trait SensitivityParse {
    fn from_str_cfg(value: &str) -> Result<SensitivityScope, String>;
}

impl SensitivityParse for SensitivityScope {
    fn from_str_cfg(value: &str) -> Result<SensitivityScope, String> {
        value
            .parse()
            .map_err(|_| format!("invalid sensitivity scope '{value}'"))
    }
}

/// Default include patterns for newly approved roots.
#[must_use]
pub fn default_include_patterns() -> Vec<String> {
    vec![
        "**/*.md".into(),
        "**/*.markdown".into(),
        "**/*.txt".into(),
        "**/*.text".into(),
        "**/*.rs".into(),
        "**/*.py".into(),
        "**/*.ts".into(),
        "**/*.js".into(),
        "**/*.toml".into(),
        "**/*.yaml".into(),
        "**/*.yml".into(),
        "**/*.json".into(),
    ]
}

/// Fingerprints configuration fields that affect derived indexing behavior.
#[must_use]
pub fn config_fingerprint(root: &Root) -> String {
    let payload = serde_json::json!({
        "include": root.include_patterns,
        "exclude": root.exclude_patterns,
        "sensitivity": root.sensitivity_tags.iter().map(|tag| {
            serde_json::json!({
                "pattern": tag.pattern,
                "scope": tag.scope.as_str(),
            })
        }).collect::<Vec<_>>(),
        "follow_symlinks": root.follow_symlinks,
    });
    blake3_hex(payload.to_string().as_bytes()).0
}

/// Creates configuration and data directories and writes a default config if absent.
///
/// # Errors
///
/// Returns path or I/O failures. Existing valid configuration is left unchanged.
pub fn init_installation(paths: &AppPaths) -> Result<(AppConfig, bool), ConfigError> {
    paths.ensure_layout()?;
    if paths.config_file.exists() {
        let config = AppConfig::load(&paths.config_file)?;
        return Ok((config, false));
    }
    let config = AppConfig::default_for(paths);
    config.save(&paths.config_file)?;
    Ok((config, true))
}

/// Approves a new root after validating the filesystem path.
///
/// # Errors
///
/// Returns configuration or filesystem validation errors.
pub fn add_root(
    config: &mut AppConfig,
    path: &Path,
    name: Option<String>,
) -> Result<RootConfig, ConfigError> {
    if !path.exists() {
        return Err(ConfigError::MissingPath(path.to_path_buf()));
    }
    let metadata = fs::metadata(path).map_err(|error| ConfigError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if !metadata.is_dir() {
        return Err(ConfigError::NotDirectory(path.to_path_buf()));
    }
    let canonical = fs::canonicalize(path).map_err(|error| ConfigError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let canonical_text = canonical.display().to_string();
    if config.roots.iter().any(|root| root.path == canonical_text) {
        return Err(ConfigError::DuplicateRootPath(canonical));
    }
    let display_name = name.unwrap_or_else(|| {
        canonical
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "root".into())
    });
    if config.roots.iter().any(|root| root.name == display_name) {
        return Err(ConfigError::DuplicateRootName(display_name));
    }
    let entry = RootConfig::new_approved(RootId::new(), display_name, &canonical);
    config.roots.push(entry.clone());
    config.validate()?;
    Ok(entry)
}

/// Removes a root from configuration by opaque ID.
///
/// # Errors
///
/// Returns [`ConfigError::RootNotFound`] when the ID is absent.
pub fn remove_root(config: &mut AppConfig, root_id: &str) -> Result<RootConfig, ConfigError> {
    let position = config
        .roots
        .iter()
        .position(|root| root.id == root_id)
        .ok_or_else(|| ConfigError::RootNotFound(root_id.to_owned()))?;
    Ok(config.roots.remove(position))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn init_is_idempotent_and_starts_without_roots() {
        let temp = TempDir::new().unwrap();
        let paths = AppPaths::for_base(temp.path());
        let (first, created) = init_installation(&paths).unwrap();
        assert!(created);
        assert!(first.roots.is_empty());
        let (second, created_again) = init_installation(&paths).unwrap();
        assert!(!created_again);
        assert_eq!(first, second);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            r#"
[general]
database_path = "/tmp/db"
mystery = true
"#,
        )
        .unwrap();
        let error = AppConfig::load(&path).unwrap_err();
        assert!(matches!(error, ConfigError::UnknownField(_)));
    }

    #[test]
    fn add_root_rejects_duplicates() {
        let temp = TempDir::new().unwrap();
        let root_dir = temp.path().join("notes");
        fs::create_dir_all(&root_dir).unwrap();
        let mut config = AppConfig::default_for(&AppPaths::for_base(temp.path()));
        add_root(&mut config, &root_dir, Some("notes".into())).unwrap();
        let error = add_root(&mut config, &root_dir, Some("other".into())).unwrap_err();
        assert!(matches!(error, ConfigError::DuplicateRootPath(_)));
    }
}
