use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::platform;
use crate::{AppError, ErrorCode, Paths, Result};

pub(super) trait ClientConfigFilesystem {
    fn exists(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> Result<String>;
    fn write_private(&self, path: &Path, data: &[u8], label: &str) -> Result<()>;
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn copy(&self, source: &Path, destination: &Path) -> Result<()>;
    fn remove_file(&self, path: &Path) -> Result<()>;
}

pub(super) struct SystemClientConfigFilesystem;

impl ClientConfigFilesystem for SystemClientConfigFilesystem {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path)
            .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot read client configuration"))
    }

    fn write_private(&self, path: &Path, data: &[u8], label: &str) -> Result<()> {
        write_system_private(path, data, label)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path).map_err(|_| {
            AppError::new(ErrorCode::StorageError, "cannot create client config directory")
        })?;
        platform::ensure_private_directory(path).map_err(|_| {
            AppError::new(ErrorCode::StorageError, "cannot protect client config directory")
        })
    }

    fn copy(&self, source: &Path, destination: &Path) -> Result<()> {
        fs::copy(source, destination)
            .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot copy client config"))?;
        platform::protect_file(destination).map_err(|_| {
            AppError::new(ErrorCode::StorageError, "cannot protect client config backup")
        })
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => {
                Err(AppError::new(ErrorCode::StorageError, "cannot roll back client configuration"))
            }
        }
    }
}

pub(super) struct ClientFiles {
    pub(super) codex: PathBuf,
    pub(super) claude_mcp: PathBuf,
    pub(super) claude_settings: PathBuf,
    pub(super) opencode: PathBuf,
}

impl ClientFiles {
    pub(super) fn discover() -> Result<Self> {
        Ok(Self::from_home_with(&home()?, &SystemClientConfigFilesystem))
    }

    #[cfg(test)]
    pub(super) fn from_home(home: &Path) -> Self {
        Self::from_home_with(home, &SystemClientConfigFilesystem)
    }

    fn from_home_with(home: &Path, filesystem: &dyn ClientConfigFilesystem) -> Self {
        let directory = home.join(".config/opencode");
        let jsonc = directory.join("opencode.jsonc");
        let json = directory.join("opencode.json");
        Self {
            codex: home.join(".codex/config.toml"),
            claude_mcp: home.join(".claude.json"),
            claude_settings: home.join(".claude/settings.json"),
            opencode: if filesystem.exists(&jsonc) || !filesystem.exists(&json) {
                jsonc
            } else {
                json
            },
        }
    }
}

pub(super) fn read_json(path: &Path, json5_allowed: bool) -> Result<Value> {
    read_json_with(&SystemClientConfigFilesystem, path, json5_allowed)
}

pub(super) fn exists(path: &Path) -> bool {
    SystemClientConfigFilesystem.exists(path)
}

pub(super) fn read_text(path: &Path) -> Result<String> {
    SystemClientConfigFilesystem.read_to_string(path)
}

pub(super) fn read_json_with(
    filesystem: &dyn ClientConfigFilesystem,
    path: &Path,
    json5_allowed: bool,
) -> Result<Value> {
    if !filesystem.exists(path) {
        return Ok(Value::Object(Map::new()));
    }
    let content = filesystem.read_to_string(path)?;
    let value: Value = if json5_allowed {
        json5::from_str(&content).map_err(|_| {
            AppError::new(ErrorCode::ConfigInvalid, "client configuration is invalid")
        })?
    } else {
        serde_json::from_str(&content).map_err(|_| {
            AppError::new(ErrorCode::ConfigInvalid, "client configuration is invalid")
        })?
    };
    if value.is_object() {
        Ok(value)
    } else {
        Err(AppError::new(ErrorCode::ConfigInvalid, "client configuration root must be an object"))
    }
}

pub(super) fn write_json(path: &Path, value: &Value) -> Result<()> {
    write_json_with(&SystemClientConfigFilesystem, path, value)
}

pub(super) fn write_json_with(
    filesystem: &dyn ClientConfigFilesystem,
    path: &Path,
    value: &Value,
) -> Result<()> {
    let data = serde_json::to_vec_pretty(value)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "cannot serialize client config"))?;
    let mut document = data;
    document.push(b'\n');
    filesystem.write_private(path, &document, "client configuration")
}

pub(super) fn write_private(path: &Path, data: &[u8], label: &str) -> Result<()> {
    SystemClientConfigFilesystem.write_private(path, data, label)
}

fn write_system_private(path: &Path, data: &[u8], label: &str) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        AppError::new(ErrorCode::ConfigInvalid, "client configuration path is invalid")
    })?;
    fs::create_dir_all(parent).map_err(|_| {
        AppError::new(ErrorCode::StorageError, "cannot create client config directory")
    })?;
    platform::atomic_write_in_existing_directory(path, data)
        .map_err(|_| AppError::new(ErrorCode::StorageError, format!("cannot write {label}")))
}

pub(super) fn backup(paths: &Paths, source: &Path, label: &str) -> Result<Option<PathBuf>> {
    backup_with(&SystemClientConfigFilesystem, paths, source, label)
}

pub(super) fn backup_with(
    filesystem: &dyn ClientConfigFilesystem,
    paths: &Paths,
    source: &Path,
    label: &str,
) -> Result<Option<PathBuf>> {
    if !filesystem.exists(source) {
        return Ok(None);
    }
    let directory = paths.support.join("Client Config Backups");
    filesystem.create_dir_all(&directory)?;
    let destination =
        directory.join(format!("{label}-{}.bak", chrono::Utc::now().timestamp_millis()));
    filesystem.copy(source, &destination)?;
    Ok(Some(destination))
}

pub(super) fn restore(destination: &Path, backup: Option<&Path>) -> Result<()> {
    restore_with(&SystemClientConfigFilesystem, destination, backup)
}

pub(super) fn restore_with(
    filesystem: &dyn ClientConfigFilesystem,
    destination: &Path,
    backup: Option<&Path>,
) -> Result<()> {
    match backup {
        Some(source) => filesystem.copy(source, destination),
        None => filesystem.remove_file(destination),
    }
}

pub(super) fn object_entry<'a>(
    document: &'a mut Value,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    let root = document.as_object_mut().ok_or_else(config_shape)?;
    let entry = root.entry(key).or_insert_with(|| Value::Object(Map::new()));
    entry.as_object_mut().ok_or_else(config_shape)
}

#[cfg(test)]
pub(super) fn array_entry<'a>(
    document: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Vec<Value>> {
    let entry = document.entry(key).or_insert_with(|| Value::Array(Vec::new()));
    entry.as_array_mut().ok_or_else(config_shape)
}

pub(super) fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| AppError::new(ErrorCode::ConfigInvalid, "executable path is not UTF-8"))
}

pub(super) fn paths_to_strings(paths: impl Iterator<Item = PathBuf>) -> Vec<String> {
    paths.map(|path| path.to_string_lossy().into_owned()).collect()
}

fn config_shape() -> AppError {
    AppError::new(ErrorCode::ConfigInvalid, "client configuration has an unsupported shape")
}

fn home() -> Result<PathBuf> {
    dirs::home_dir()
        .ok_or_else(|| AppError::new(ErrorCode::ConfigInvalid, "cannot determine home directory"))
}
