use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{ProfileId, VpnProfile},
    error::AppError,
    platform::{
        create_private_file, ensure_private_directory, replace_file_atomically, sync_directory,
    },
    vpn::parser::{ExternalFileReference, ParsedOvpnConfig},
};

const STORE_VERSION: u32 = 1;
const PROFILES_DIRECTORY: &str = "profiles";
const METADATA_FILE: &str = "metadata.json";
const IMPORTED_CONFIG_FILE: &str = "config.ovpn";
const ORIGINAL_CONFIG_FILE: &str = "original.ovpn";
const STAGING_PREFIX: &str = ".import-";
const METADATA_TEMPORARY_PREFIX: &str = ".metadata-";
const METADATA_TEMPORARY_SUFFIX: &str = ".tmp";
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ASSET_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct StoredProfile {
    version: u32,
    profile: VpnProfile,
}

/// JSON-backed store for app-owned OpenVPN profiles.
pub struct ProfileStore {
    profiles_root: PathBuf,
}

impl ProfileStore {
    pub fn new(app_data_dir: PathBuf) -> Result<Self, AppError> {
        let profiles_root = app_data_dir.join(PROFILES_DIRECTORY);
        ensure_private_directory(&profiles_root)?;

        let store = Self { profiles_root };
        store.cleanup_staging_directories()?;
        Ok(store)
    }

    pub fn import_profile(&self, source_path: &Path) -> Result<VpnProfile, AppError> {
        validate_ovpn_extension(source_path)?;

        let source_path = fs::canonicalize(source_path)?;
        let source_metadata = fs::metadata(&source_path)?;
        if !source_metadata.is_file() {
            return Err(AppError::ConfigInvalid {
                reason: "selected OpenVPN configuration is not a regular file".to_owned(),
            });
        }
        if source_metadata.len() > MAX_CONFIG_BYTES {
            return Err(AppError::ConfigInvalid {
                reason: "OpenVPN configuration exceeds the import size limit".to_owned(),
            });
        }

        let source_bytes = fs::read(&source_path)?;
        let source_text =
            std::str::from_utf8(&source_bytes).map_err(|_| AppError::ConfigInvalid {
                reason: "OpenVPN configuration must be UTF-8 text".to_owned(),
            })?;
        let parsed = ParsedOvpnConfig::parse(source_text)?;

        let existing_profiles = self.list_profiles()?;
        let base_name = source_path
            .file_stem()
            .and_then(OsStr::to_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("OpenVPN Profile");
        let profile_name = unique_profile_name(base_name, &existing_profiles);

        let profile_id = ProfileId::new(Uuid::new_v4().to_string())?;
        let final_directory = self.profile_directory(&profile_id);
        let staging_directory = self
            .profiles_root
            .join(format!("{STAGING_PREFIX}{}", profile_id.as_str()));
        ensure_private_directory(&staging_directory)?;

        let import_result = Self::import_into_staging(
            &source_path,
            &source_bytes,
            &parsed,
            profile_id,
            profile_name,
            &staging_directory,
            &final_directory,
        );

        match import_result {
            Ok(profile) => Ok(profile),
            Err(error) => {
                let _ = fs::remove_dir_all(&staging_directory);
                Err(error)
            }
        }
    }

    pub fn list_profiles(&self) -> Result<Vec<VpnProfile>, AppError> {
        let mut profiles = Vec::new();

        for entry in fs::read_dir(&self.profiles_root)? {
            let entry = entry?;
            let file_name = entry.file_name();
            if file_name.to_string_lossy().starts_with('.') {
                continue;
            }

            let file_type = entry.file_type()?;
            if !file_type.is_dir() || file_type.is_symlink() {
                return Err(AppError::ProfileStoreCorrupted {
                    reason: "profile store contains an unexpected entry".to_owned(),
                });
            }

            let profile = read_stored_profile(&entry.path())?;
            let directory_id =
                file_name
                    .to_str()
                    .ok_or_else(|| AppError::ProfileStoreCorrupted {
                        reason: "profile directory name is not valid UTF-8".to_owned(),
                    })?;
            if profile.id.as_str() != directory_id {
                return Err(AppError::ProfileStoreCorrupted {
                    reason: "profile metadata ID does not match its directory".to_owned(),
                });
            }
            profiles.push(profile);
        }

        profiles.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        });
        Ok(profiles)
    }

    pub fn get_profile(&self, profile_id: &ProfileId) -> Result<VpnProfile, AppError> {
        let profile_directory = self.profile_directory(profile_id);
        let metadata = match fs::symlink_metadata(&profile_directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(AppError::ProfileNotFound {
                    profile_id: profile_id.to_string(),
                });
            }
            Err(error) => return Err(error.into()),
        };

        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(AppError::ProfileStoreCorrupted {
                reason: "profile path is not a directory".to_owned(),
            });
        }

        let profile = read_stored_profile(&profile_directory)?;
        if &profile.id != profile_id {
            return Err(AppError::ProfileStoreCorrupted {
                reason: "profile metadata ID does not match its directory".to_owned(),
            });
        }

        Ok(profile)
    }

    pub fn delete_profile(&self, profile_id: &ProfileId) -> Result<(), AppError> {
        let profile_directory = self.profile_directory(profile_id);
        self.get_profile(profile_id)?;
        fs::remove_dir_all(profile_directory)?;
        Ok(())
    }

    pub fn update_profile(
        &self,
        profile_id: &ProfileId,
        name: String,
        ignore_redirect_gateway: bool,
    ) -> Result<VpnProfile, AppError> {
        let profile_directory = self.profile_directory(profile_id);
        let mut profile = self.get_profile(profile_id)?;
        profile.update_editable_settings(name, ignore_redirect_gateway)?;
        persist_profile_metadata(&profile_directory, &profile)?;
        Ok(profile)
    }

    fn import_into_staging(
        source_path: &Path,
        source_bytes: &[u8],
        parsed: &ParsedOvpnConfig,
        profile_id: ProfileId,
        profile_name: String,
        staging_directory: &Path,
        final_directory: &Path,
    ) -> Result<VpnProfile, AppError> {
        write_private_file(&staging_directory.join(ORIGINAL_CONFIG_FILE), source_bytes)?;

        let source_directory = source_path
            .parent()
            .ok_or_else(|| AppError::ConfigInvalid {
                reason: "OpenVPN configuration has no parent directory".to_owned(),
            })?;
        let mut replacements = HashMap::new();

        for (index, reference) in parsed.external_files().iter().enumerate() {
            let source_asset = resolve_external_file(source_directory, reference)?;
            let asset_name = asset_file_name(reference, index + 1);
            copy_private_file(
                &source_asset,
                &staging_directory.join(&asset_name),
                MAX_ASSET_BYTES,
            )?;
            replacements.insert(reference.line_index, asset_name);
        }

        let imported_config = parsed.render_imported(&replacements)?;
        write_private_file(
            &staging_directory.join(IMPORTED_CONFIG_FILE),
            imported_config.as_bytes(),
        )?;

        let mut profile = VpnProfile::new(
            profile_id,
            profile_name,
            final_directory.join(IMPORTED_CONFIG_FILE),
        )?;
        if let Some(remote) = &parsed.remote {
            profile.server_host = Some(remote.host.clone());
            profile.server_port = remote.port;
            profile.protocol = parsed.protocol.clone().or_else(|| remote.protocol.clone());
        } else {
            profile.protocol = parsed.protocol.clone();
        }

        let stored = StoredProfile {
            version: STORE_VERSION,
            profile: profile.clone(),
        };
        let metadata_bytes = serde_json::to_vec_pretty(&stored).map_err(|error| {
            AppError::ProfileStoreCorrupted {
                reason: format!("failed to serialize profile metadata: {error}"),
            }
        })?;
        write_private_file(&staging_directory.join(METADATA_FILE), &metadata_bytes)?;

        fs::rename(staging_directory, final_directory)?;
        Ok(profile)
    }

    fn profile_directory(&self, profile_id: &ProfileId) -> PathBuf {
        self.profiles_root.join(profile_id.as_str())
    }

    fn cleanup_staging_directories(&self) -> Result<(), AppError> {
        for entry in fs::read_dir(&self.profiles_root)? {
            let entry = entry?;
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with(STAGING_PREFIX)
            {
                continue;
            }

            let file_type = entry.file_type()?;
            if file_type.is_dir() && !file_type.is_symlink() {
                fs::remove_dir_all(entry.path())?;
            }
        }
        Ok(())
    }
}

fn validate_ovpn_extension(path: &Path) -> Result<(), AppError> {
    let is_ovpn = path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ovpn"));

    if !is_ovpn {
        return Err(AppError::ConfigInvalid {
            reason: "selected file must use the .ovpn extension".to_owned(),
        });
    }
    Ok(())
}

fn unique_profile_name(base_name: &str, existing_profiles: &[VpnProfile]) -> String {
    let existing_names = existing_profiles
        .iter()
        .map(|profile| profile.name.to_lowercase())
        .collect::<std::collections::HashSet<_>>();

    if !existing_names.contains(&base_name.to_lowercase()) {
        return base_name.to_owned();
    }

    for suffix in 2_u32..=u32::MAX {
        let candidate = format!("{base_name} ({suffix})");
        if !existing_names.contains(&candidate.to_lowercase()) {
            return candidate;
        }
    }

    format!("{base_name} ({})", Uuid::new_v4())
}

fn resolve_external_file(
    source_directory: &Path,
    reference: &ExternalFileReference,
) -> Result<PathBuf, AppError> {
    let candidate = if reference.path.is_absolute() {
        reference.path.clone()
    } else {
        source_directory.join(&reference.path)
    };

    let canonical = fs::canonicalize(&candidate).map_err(|_| AppError::ConfigInvalid {
        reason: format!(
            "referenced {} file at line {} is missing",
            reference.directive, reference.line_number
        ),
    })?;
    let metadata = fs::metadata(&canonical)?;
    if !metadata.is_file() {
        return Err(AppError::ConfigInvalid {
            reason: format!(
                "referenced {} path at line {} is not a regular file",
                reference.directive, reference.line_number
            ),
        });
    }
    Ok(canonical)
}

fn asset_file_name(reference: &ExternalFileReference, ordinal: usize) -> String {
    let base = reference
        .directive
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .collect::<String>();
    let extension = reference
        .path
        .extension()
        .and_then(OsStr::to_str)
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 10
                && extension
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        });

    match extension {
        Some(extension) => format!("{base}-{ordinal:02}.{extension}"),
        None => format!("{base}-{ordinal:02}.pem"),
    }
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    let mut file = create_private_file(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn copy_private_file(source: &Path, destination: &Path, max_bytes: u64) -> Result<(), AppError> {
    let metadata = fs::metadata(source)?;
    if metadata.len() > max_bytes {
        return Err(AppError::ConfigInvalid {
            reason: "referenced profile asset exceeds the import size limit".to_owned(),
        });
    }

    let mut source_file = File::open(source)?;
    let mut destination_file = create_private_file(destination)?;
    let copied = io::copy(
        &mut Read::by_ref(&mut source_file).take(max_bytes + 1),
        &mut destination_file,
    )?;
    if copied > max_bytes {
        return Err(AppError::ConfigInvalid {
            reason: "referenced profile asset exceeds the import size limit".to_owned(),
        });
    }
    destination_file.sync_all()?;
    Ok(())
}

fn read_stored_profile(profile_directory: &Path) -> Result<VpnProfile, AppError> {
    let metadata_path = profile_directory.join(METADATA_FILE);
    let bytes = fs::read(&metadata_path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            AppError::ProfileStoreCorrupted {
                reason: "profile metadata is missing".to_owned(),
            }
        } else {
            error.into()
        }
    })?;
    let stored = serde_json::from_slice::<StoredProfile>(&bytes).map_err(|error| {
        AppError::ProfileStoreCorrupted {
            reason: format!("profile metadata is invalid: {error}"),
        }
    })?;

    if stored.version != STORE_VERSION {
        return Err(AppError::ProfileStoreCorrupted {
            reason: format!("unsupported profile metadata version: {}", stored.version),
        });
    }

    let expected_config_path = profile_directory.join(IMPORTED_CONFIG_FILE);
    if stored.profile.config_path != expected_config_path {
        return Err(AppError::ProfileStoreCorrupted {
            reason: "profile configuration path escaped its profile directory".to_owned(),
        });
    }

    let config_metadata = fs::symlink_metadata(&expected_config_path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            AppError::ProfileStoreCorrupted {
                reason: "imported OpenVPN configuration is missing".to_owned(),
            }
        } else {
            error.into()
        }
    })?;
    if !config_metadata.is_file() || config_metadata.file_type().is_symlink() {
        return Err(AppError::ProfileStoreCorrupted {
            reason: "imported OpenVPN configuration is not a regular file".to_owned(),
        });
    }

    Ok(stored.profile)
}

fn persist_profile_metadata(
    profile_directory: &Path,
    profile: &VpnProfile,
) -> Result<(), AppError> {
    let stored = StoredProfile {
        version: STORE_VERSION,
        profile: profile.clone(),
    };
    let contents =
        serde_json::to_vec_pretty(&stored).map_err(|error| AppError::ProfileStoreCorrupted {
            reason: format!("failed to serialize profile metadata: {error}"),
        })?;
    let temporary_path = profile_directory.join(format!(
        "{METADATA_TEMPORARY_PREFIX}{}{METADATA_TEMPORARY_SUFFIX}",
        Uuid::new_v4()
    ));
    let metadata_path = profile_directory.join(METADATA_FILE);

    let write_result = (|| -> Result<(), AppError> {
        let mut temporary_file = create_private_file(&temporary_path)?;
        temporary_file.write_all(&contents)?;
        temporary_file.sync_all()?;
        drop(temporary_file);

        replace_file_atomically(&temporary_path, &metadata_path)?;
        sync_directory(profile_directory)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::TempDir;

    use super::ProfileStore;

    fn write_fixture(directory: &Path, ca_contents: &str) {
        fs::create_dir_all(directory).expect("fixture directory should be created");
        fs::write(directory.join("ca.crt"), ca_contents).expect("CA should be written");
        fs::write(directory.join("client.crt"), "client certificate")
            .expect("certificate should be written");
        fs::write(directory.join("client.key"), "private key").expect("key should be written");
        fs::write(directory.join("credentials.txt"), "username\npassword\n")
            .expect("credentials should be written");
        fs::write(
            directory.join("office.ovpn"),
            "client\nproto tcp\nremote vpn.example 1194\nca ca.crt\ncert client.crt\nkey client.key\nauth-user-pass credentials.txt\n",
        )
        .expect("config should be written");
    }

    #[test]
    fn imports_profiles_into_isolated_directories() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let source_a = workspace.path().join("source-a");
        let source_b = workspace.path().join("source-b");
        write_fixture(&source_a, "CA A");
        write_fixture(&source_b, "CA B");

        let store =
            ProfileStore::new(workspace.path().join("app-data")).expect("store should initialize");
        let profile_a = store
            .import_profile(&source_a.join("office.ovpn"))
            .expect("first profile should import");
        let profile_b = store
            .import_profile(&source_b.join("office.ovpn"))
            .expect("second profile should import");

        assert_ne!(profile_a.id, profile_b.id);
        assert_eq!(profile_a.name, "office");
        assert_eq!(profile_b.name, "office (2)");
        assert_eq!(profile_a.server_host.as_deref(), Some("vpn.example"));
        assert_eq!(profile_a.server_port, Some(1194));
        assert_eq!(profile_a.protocol.as_deref(), Some("tcp"));

        let directory_a = profile_a
            .config_path
            .parent()
            .expect("profile should have a directory");
        let directory_b = profile_b
            .config_path
            .parent()
            .expect("profile should have a directory");
        assert_ne!(directory_a, directory_b);
        assert_eq!(
            fs::read_to_string(directory_a.join("ca-01.crt")).expect("CA A should exist"),
            "CA A"
        );
        assert_eq!(
            fs::read_to_string(directory_b.join("ca-01.crt")).expect("CA B should exist"),
            "CA B"
        );
        assert!(directory_a.join("original.ovpn").is_file());
        assert_eq!(
            fs::read_to_string(directory_a.join("original.ovpn"))
                .expect("original config should be readable"),
            fs::read_to_string(source_a.join("office.ovpn"))
                .expect("source config should be readable")
        );

        let imported_config =
            fs::read_to_string(&profile_a.config_path).expect("config should be readable");
        assert!(imported_config.contains("ca \"ca-01.crt\""));
        assert!(imported_config.contains("cert \"cert-02.crt\""));
        assert!(imported_config.contains("key \"key-03.key\""));
        assert!(imported_config.contains("auth-user-pass\n"));
        assert!(!imported_config.contains("credentials.txt"));
        assert!(!directory_a.join("credentials.txt").exists());
    }

    #[test]
    fn lists_gets_and_deletes_profiles() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let source = workspace.path().join("source");
        write_fixture(&source, "CA");
        let store =
            ProfileStore::new(workspace.path().join("app-data")).expect("store should initialize");
        let imported = store
            .import_profile(&source.join("office.ovpn"))
            .expect("profile should import");

        let profiles = store.list_profiles().expect("profiles should list");
        assert_eq!(profiles, vec![imported.clone()]);
        assert_eq!(
            store
                .get_profile(&imported.id)
                .expect("profile should be found"),
            imported
        );

        let profile_directory = imported
            .config_path
            .parent()
            .expect("profile should have a directory")
            .to_path_buf();
        store
            .delete_profile(&imported.id)
            .expect("profile should delete");

        assert!(!profile_directory.exists());
        assert!(store
            .list_profiles()
            .expect("profiles should list")
            .is_empty());
        assert!(store.get_profile(&imported.id).is_err());
    }

    #[test]
    fn updates_and_persists_editable_profile_settings() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let source = workspace.path().join("source");
        write_fixture(&source, "CA");
        let app_data = workspace.path().join("app-data");
        let store = ProfileStore::new(app_data.clone()).expect("store should initialize");
        let imported = store
            .import_profile(&source.join("office.ovpn"))
            .expect("profile should import");

        let updated = store
            .update_profile(&imported.id, "  Production VPN  ".to_owned(), false)
            .expect("profile should update");

        assert_eq!(updated.name, "Production VPN");
        assert!(!updated.ignore_redirect_gateway);
        assert!(updated.updated_at >= imported.updated_at);
        assert_eq!(updated.config_path, imported.config_path);

        let reloaded = ProfileStore::new(app_data).expect("store should reload");
        assert_eq!(
            reloaded
                .get_profile(&imported.id)
                .expect("updated profile should load"),
            updated
        );
    }

    #[test]
    fn failed_import_does_not_leave_partial_profile() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let source = workspace.path().join("source");
        fs::create_dir_all(&source).expect("source should be created");
        fs::write(
            source.join("broken.ovpn"),
            "client\nremote vpn.example 1194\nca missing.crt\n",
        )
        .expect("config should be written");
        let app_data = workspace.path().join("app-data");
        let store = ProfileStore::new(app_data.clone()).expect("store should initialize");

        assert!(store.import_profile(&source.join("broken.ovpn")).is_err());
        assert!(store
            .list_profiles()
            .expect("profiles should list")
            .is_empty());

        let entries = fs::read_dir(app_data.join("profiles"))
            .expect("profile root should exist")
            .count();
        assert_eq!(entries, 0);
    }

    #[cfg(unix)]
    #[test]
    fn imported_keys_and_profile_directories_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let workspace = TempDir::new().expect("temporary directory should be created");
        let source = workspace.path().join("source");
        write_fixture(&source, "CA");
        let store =
            ProfileStore::new(workspace.path().join("app-data")).expect("store should initialize");
        let profile = store
            .import_profile(&source.join("office.ovpn"))
            .expect("profile should import");
        let profile_directory = profile
            .config_path
            .parent()
            .expect("profile should have a directory");

        let directory_mode = fs::metadata(profile_directory)
            .expect("directory metadata should exist")
            .permissions()
            .mode();
        let key_mode = fs::metadata(profile_directory.join("key-03.key"))
            .expect("key metadata should exist")
            .permissions()
            .mode();

        assert_eq!(directory_mode & 0o077, 0);
        assert_eq!(key_mode & 0o077, 0);
    }
}
