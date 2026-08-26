use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::AppError,
    platform::{
        create_private_file, ensure_private_directory, replace_file_atomically, sync_directory,
    },
};

const SETTINGS_VERSION: u32 = 1;
const SETTINGS_FILE: &str = "settings.json";
const TEMPORARY_PREFIX: &str = ".settings-";
const TEMPORARY_SUFFIX: &str = ".tmp";
const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub openvpn_executable: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct StoredSettings {
    version: u32,
    settings: AppSettings,
}

pub struct SettingsStore {
    settings_path: PathBuf,
    settings: AppSettings,
}

impl SettingsStore {
    pub fn new(app_data_directory: PathBuf) -> Result<Self, AppError> {
        ensure_private_directory(&app_data_directory)?;
        cleanup_temporary_files(&app_data_directory)?;

        let settings_path = app_data_directory.join(SETTINGS_FILE);
        let settings = match fs::symlink_metadata(&settings_path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(AppError::SettingsCorrupted {
                        reason: "settings path is not a regular file".to_owned(),
                    });
                }
                if metadata.len() > MAX_SETTINGS_BYTES {
                    return Err(AppError::SettingsCorrupted {
                        reason: "settings file exceeds the size limit".to_owned(),
                    });
                }
                read_settings(&settings_path)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppSettings::default(),
            Err(error) => return Err(error.into()),
        };

        Ok(Self {
            settings_path,
            settings,
        })
    }

    #[must_use]
    pub fn get(&self) -> AppSettings {
        self.settings.clone()
    }

    pub fn set_openvpn_executable(
        &mut self,
        path: Option<PathBuf>,
    ) -> Result<AppSettings, AppError> {
        let next_settings = AppSettings {
            openvpn_executable: path,
        };
        self.persist(&next_settings)?;
        self.settings = next_settings;
        Ok(self.settings.clone())
    }

    fn persist(&self, settings: &AppSettings) -> Result<(), AppError> {
        let stored = StoredSettings {
            version: SETTINGS_VERSION,
            settings: settings.clone(),
        };
        let contents =
            serde_json::to_vec_pretty(&stored).map_err(|error| AppError::SettingsCorrupted {
                reason: format!("failed to serialize settings: {error}"),
            })?;
        let parent = self
            .settings_path
            .parent()
            .ok_or_else(|| AppError::SettingsCorrupted {
                reason: "settings path has no parent directory".to_owned(),
            })?;
        let temporary_path = parent.join(format!(
            "{TEMPORARY_PREFIX}{}{TEMPORARY_SUFFIX}",
            Uuid::new_v4()
        ));

        let write_result = (|| -> Result<(), AppError> {
            let mut temporary_file = create_private_file(&temporary_path)?;
            temporary_file.write_all(&contents)?;
            temporary_file.sync_all()?;
            drop(temporary_file);

            replace_file_atomically(&temporary_path, &self.settings_path)?;
            sync_directory(parent)?;
            Ok(())
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        write_result
    }
}

fn read_settings(path: &Path) -> Result<AppSettings, AppError> {
    let contents = fs::read(path)?;
    let stored = serde_json::from_slice::<StoredSettings>(&contents).map_err(|error| {
        AppError::SettingsCorrupted {
            reason: format!("settings JSON is invalid: {error}"),
        }
    })?;
    if stored.version != SETTINGS_VERSION {
        return Err(AppError::SettingsCorrupted {
            reason: format!("unsupported settings version: {}", stored.version),
        });
    }
    Ok(stored.settings)
}

fn cleanup_temporary_files(directory: &Path) -> Result<(), AppError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(TEMPORARY_PREFIX) || !name.ends_with(TEMPORARY_SUFFIX) {
            continue;
        }

        let file_type = entry.file_type()?;
        if file_type.is_file() && !file_type.is_symlink() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use tempfile::TempDir;

    use super::SettingsStore;

    #[test]
    fn returns_defaults_when_settings_do_not_exist() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let store = SettingsStore::new(workspace.path().join("app-data"))
            .expect("settings store should initialize");

        assert_eq!(store.get().openvpn_executable, None);
    }

    #[test]
    fn atomically_persists_and_reloads_custom_path() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let app_data = workspace.path().join("app-data");
        let executable = PathBuf::from("/opt/openvpn/bin/openvpn");
        let mut store =
            SettingsStore::new(app_data.clone()).expect("settings store should initialize");

        let updated = store
            .set_openvpn_executable(Some(executable.clone()))
            .expect("settings should persist");
        assert_eq!(updated.openvpn_executable, Some(executable.clone()));

        let reloaded = SettingsStore::new(app_data.clone()).expect("settings should reload");
        assert_eq!(reloaded.get().openvpn_executable, Some(executable));

        let replacement = PathBuf::from("/usr/local/sbin/openvpn");
        store
            .set_openvpn_executable(Some(replacement.clone()))
            .expect("existing settings should be replaced");
        let reloaded = SettingsStore::new(app_data.clone()).expect("settings should reload");
        assert_eq!(reloaded.get().openvpn_executable, Some(replacement));

        let temporary_files = fs::read_dir(app_data)
            .expect("app data should be readable")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".settings-")
            })
            .count();
        assert_eq!(temporary_files, 0);
    }

    #[test]
    fn rejects_corrupted_settings() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let app_data = workspace.path().join("app-data");
        fs::create_dir_all(&app_data).expect("app data should be created");
        fs::write(app_data.join("settings.json"), "not json")
            .expect("invalid settings should be written");

        let error = SettingsStore::new(app_data)
            .err()
            .expect("corrupted settings should fail");
        assert_eq!(error.code(), "SETTINGS_CORRUPTED");
        assert_eq!(error.to_payload().details, None);
    }

    #[cfg(unix)]
    #[test]
    fn settings_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let workspace = TempDir::new().expect("temporary directory should be created");
        let app_data = workspace.path().join("app-data");
        let mut store =
            SettingsStore::new(app_data.clone()).expect("settings store should initialize");
        store
            .set_openvpn_executable(Some(PathBuf::from("/opt/openvpn")))
            .expect("settings should persist");

        let mode = fs::metadata(app_data.join("settings.json"))
            .expect("settings metadata should exist")
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0);
    }
}
