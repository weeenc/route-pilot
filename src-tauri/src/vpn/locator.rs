use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenVpnSource {
    Bundled,
    Custom,
    Path,
    Common,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocatedOpenVpn {
    pub path: PathBuf,
    pub source: OpenVpnSource,
}

/// Finds an OpenVPN 2.x executable without invoking a shell or executing the
/// candidate binary.
pub struct OpenVpnLocator {
    bundled_candidates: Vec<PathBuf>,
    common_candidates: Vec<PathBuf>,
}

impl OpenVpnLocator {
    #[must_use]
    pub fn new(resource_directory: PathBuf) -> Self {
        let binary_name = binary_name();
        let platform_directory = platform_directory();
        let manifest_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        Self {
            bundled_candidates: vec![
                resource_directory
                    .join("binaries")
                    .join(platform_directory)
                    .join(binary_name),
                resource_directory.join(binary_name),
                manifest_directory
                    .join("binaries")
                    .join(platform_directory)
                    .join(binary_name),
            ],
            common_candidates: common_candidates(),
        }
    }

    pub fn locate(&self, custom_path: Option<&Path>) -> Result<LocatedOpenVpn, AppError> {
        self.locate_with_path(custom_path, env::var_os("PATH"))
    }

    pub fn validate_custom_path(&self, path: &Path) -> Result<PathBuf, AppError> {
        if !path.is_absolute() || resolved_executable(path).is_none() {
            return Err(AppError::OpenVpnInvalidExecutable {
                reason: "the selected path must be an absolute executable file".to_owned(),
            });
        }

        Ok(path.to_path_buf())
    }

    fn locate_with_path(
        &self,
        custom_path: Option<&Path>,
        path_environment: Option<OsString>,
    ) -> Result<LocatedOpenVpn, AppError> {
        let mut visited = HashSet::new();

        if let Some(located) = first_executable(
            self.bundled_candidates.iter().map(PathBuf::as_path),
            OpenVpnSource::Bundled,
            &mut visited,
        ) {
            return Ok(located);
        }

        if let Some(custom_path) = custom_path {
            if let Some(path) = resolved_executable(custom_path) {
                visited.insert(path.clone());
                return Ok(LocatedOpenVpn {
                    path,
                    source: OpenVpnSource::Custom,
                });
            }
        }

        if let Some(path_environment) = path_environment {
            let candidates = env::split_paths(&path_environment)
                .filter(|directory| !directory.as_os_str().is_empty())
                .map(|directory| directory.join(binary_name()))
                .collect::<Vec<_>>();
            if let Some(located) = first_executable(
                candidates.iter().map(PathBuf::as_path),
                OpenVpnSource::Path,
                &mut visited,
            ) {
                return Ok(located);
            }
        }

        first_executable(
            self.common_candidates.iter().map(PathBuf::as_path),
            OpenVpnSource::Common,
            &mut visited,
        )
        .ok_or(AppError::OpenVpnNotFound)
    }

    #[cfg(test)]
    fn with_candidates(bundled_candidates: Vec<PathBuf>, common_candidates: Vec<PathBuf>) -> Self {
        Self {
            bundled_candidates,
            common_candidates,
        }
    }
}

fn first_executable<'a>(
    candidates: impl Iterator<Item = &'a Path>,
    source: OpenVpnSource,
    visited: &mut HashSet<PathBuf>,
) -> Option<LocatedOpenVpn> {
    for candidate in candidates {
        let Some(path) = resolved_executable(candidate) else {
            continue;
        };
        if !visited.insert(path.clone()) {
            continue;
        }
        return Some(LocatedOpenVpn { path, source });
    }
    None
}

pub(super) fn resolved_executable(candidate: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    if !candidate
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return None;
    }

    let canonical = fs::canonicalize(candidate).ok()?;
    let metadata = fs::metadata(&canonical).ok()?;
    if !metadata.is_file() {
        return None;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }

    Some(canonical)
}

#[cfg(target_os = "windows")]
fn binary_name() -> &'static str {
    "openvpn.exe"
}

#[cfg(not(target_os = "windows"))]
fn binary_name() -> &'static str {
    "openvpn"
}

#[cfg(target_os = "windows")]
fn platform_directory() -> &'static str {
    "windows"
}

#[cfg(target_os = "macos")]
fn platform_directory() -> &'static str {
    "macos"
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_directory() -> &'static str {
    "unsupported"
}

#[cfg(target_os = "macos")]
fn common_candidates() -> Vec<PathBuf> {
    [
        "/opt/homebrew/sbin/openvpn",
        "/usr/local/sbin/openvpn",
        "/opt/local/sbin/openvpn",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

#[cfg(target_os = "windows")]
fn common_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(directory) = env::var_os(variable) {
            candidates.push(
                PathBuf::from(directory)
                    .join("OpenVPN")
                    .join("bin")
                    .join("openvpn.exe"),
            );
        }
    }
    candidates
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn common_candidates() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use std::{env, ffi::OsString, fs, path::Path};

    use tempfile::TempDir;

    use super::{OpenVpnLocator, OpenVpnSource};

    fn create_executable(path: &Path) {
        fs::write(path, b"test executable").expect("test executable should be written");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .expect("test executable permissions should be set");
        }
    }

    fn path_environment(directory: &Path) -> OsString {
        env::join_paths([directory]).expect("test PATH should be valid")
    }

    #[test]
    fn follows_bundled_custom_path_and_common_precedence() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let bundled = workspace.path().join("bundled").join(super::binary_name());
        let custom = workspace.path().join("custom").join(super::binary_name());
        let path_directory = workspace.path().join("path");
        let path_binary = path_directory.join(super::binary_name());
        let common = workspace.path().join("common").join(super::binary_name());
        for candidate in [&bundled, &custom, &path_binary, &common] {
            fs::create_dir_all(candidate.parent().expect("candidate parent should exist"))
                .expect("candidate directory should be created");
            create_executable(candidate);
        }

        let locator = OpenVpnLocator::with_candidates(vec![bundled.clone()], vec![common.clone()]);
        let path = Some(path_environment(&path_directory));

        let located = locator
            .locate_with_path(Some(&custom), path.clone())
            .expect("bundled executable should be found");
        assert_eq!(located.source, OpenVpnSource::Bundled);
        assert_eq!(
            located.path,
            fs::canonicalize(&bundled).expect("path should resolve")
        );

        fs::remove_file(&bundled).expect("bundled executable should be removed");
        let located = locator
            .locate_with_path(Some(&custom), path.clone())
            .expect("custom executable should be found");
        assert_eq!(located.source, OpenVpnSource::Custom);

        fs::remove_file(&custom).expect("custom executable should be removed");
        let located = locator
            .locate_with_path(Some(&custom), path)
            .expect("PATH executable should be found");
        assert_eq!(located.source, OpenVpnSource::Path);

        fs::remove_file(&path_binary).expect("PATH executable should be removed");
        let located = locator
            .locate_with_path(Some(&custom), None)
            .expect("common executable should be found");
        assert_eq!(located.source, OpenVpnSource::Common);
    }

    #[test]
    fn rejects_non_executable_custom_files() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let candidate = workspace.path().join("openvpn");
        fs::write(&candidate, b"not executable").expect("test file should be written");

        #[cfg(unix)]
        fs::set_permissions(&candidate, {
            use std::os::unix::fs::PermissionsExt;
            fs::Permissions::from_mode(0o600)
        })
        .expect("test permissions should be set");

        let locator = OpenVpnLocator::with_candidates(Vec::new(), Vec::new());
        assert!(locator.validate_custom_path(&candidate).is_err());
        assert!(locator
            .validate_custom_path(Path::new("relative/openvpn"))
            .is_err());
        assert!(locator.locate_with_path(Some(&candidate), None).is_err());
    }
}
