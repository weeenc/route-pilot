use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use tokio::{process::Command, sync::Mutex as AsyncMutex, time::timeout};
use uuid::Uuid;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::error::AppError;

const TAPCTL_EXECUTABLE: &str = "tapctl.exe";
const TAP_HARDWARE_ID: &str = "tap0901";
const TAP_HARDWARE_ID_WITH_ROOT: &str = "root\\tap0901";
const ADAPTER_NAME_PREFIX: &str = "RoutePilot TAP ";
const MAX_POOL_SIZE: usize = 16;
const TAPCTL_TIMEOUT: Duration = Duration::from_secs(15);

static ALLOCATION_LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
static RESERVED_ADAPTERS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Clone, PartialEq, Eq)]
struct TapAdapterInfo {
    identifier: String,
    name: String,
    hardware_id: String,
}

/// A process-local lease on one persistent RoutePilot TAP adapter.
///
/// Creating or deleting TAP-Windows6 devices forces Windows Plug and Play to
/// reconfigure the network stack. The pool keeps devices installed and only
/// prevents active OpenVPN processes from choosing the same adapter.
pub(super) struct WindowsTapAdapter {
    name: String,
}

impl WindowsTapAdapter {
    pub(super) async fn acquire(openvpn: &Path) -> Result<Self, AppError> {
        let tapctl = tapctl_path(openvpn)?;
        let _allocation = allocation_lock().lock().await;
        let adapters = list_pool_adapters(&tapctl).await?;

        if let Some(name) = reserve_available(&adapters)? {
            return Ok(Self { name });
        }

        let name = next_pool_name(&adapters).ok_or_else(|| AppError::OpenVpnStartFailed {
            reason: format!("all {MAX_POOL_SIZE} RoutePilot TAP adapters are in use"),
        })?;
        create_pool_adapter(&tapctl, &name).await?;
        reserve(&name)?;
        Ok(Self { name })
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for WindowsTapAdapter {
    fn drop(&mut self) {
        if let Ok(mut reserved) = reserved_adapters().lock() {
            reserved.remove(&self.name);
        }
    }
}

pub(crate) async fn prewarm(openvpn: &Path, desired_count: usize) -> Result<(), AppError> {
    let tapctl = tapctl_path(openvpn)?;
    let desired_count = desired_count.min(MAX_POOL_SIZE);

    loop {
        // Release this lock after each adapter so an immediate connection can
        // use the first completed device while the rest of the pool warms up.
        let _allocation = allocation_lock().lock().await;
        let adapters = list_pool_adapters(&tapctl).await?;
        if adapters.len() >= desired_count {
            return Ok(());
        }
        let name = next_pool_name(&adapters).ok_or_else(|| AppError::OpenVpnStartFailed {
            reason: "RoutePilot TAP adapter pool is full".to_owned(),
        })?;
        create_pool_adapter(&tapctl, &name).await?;
    }
}

async fn list_pool_adapters(tapctl: &Path) -> Result<Vec<TapAdapterInfo>, AppError> {
    let mut command = tapctl_command(tapctl);
    command.arg("list");
    let output = run_tapctl(command, "list Windows TAP adapters").await?;
    if !output.status.success() {
        return Err(AppError::OpenVpnStartFailed {
            reason: format!(
                "tapctl could not list Windows TAP adapters (exit code {:?})",
                output.status.code()
            ),
        });
    }

    let output = String::from_utf8_lossy(&output.stdout);
    let mut adapters = output
        .lines()
        .filter_map(parse_adapter_line)
        .filter(|adapter| {
            pool_index(&adapter.name).is_some()
                && (adapter.hardware_id.eq_ignore_ascii_case(TAP_HARDWARE_ID)
                    || adapter
                        .hardware_id
                        .eq_ignore_ascii_case(TAP_HARDWARE_ID_WITH_ROOT))
        })
        .collect::<Vec<_>>();
    adapters.sort_by_key(|adapter| pool_index(&adapter.name));
    Ok(adapters)
}

async fn create_pool_adapter(tapctl: &Path, name: &str) -> Result<(), AppError> {
    let mut command = tapctl_command(tapctl);
    command.args(["create", "--name", name, "--hwid", TAP_HARDWARE_ID]);
    let output = run_tapctl(command, "create a Windows TAP adapter").await?;
    if !output.status.success() {
        return Err(AppError::OpenVpnStartFailed {
            reason: format!(
                "tapctl could not create TAP adapter '{name}' (exit code {:?})",
                output.status.code()
            ),
        });
    }

    let created = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .and_then(parse_adapter_line);
    if !created.is_some_and(|adapter| {
        adapter.name == name
            && (adapter.hardware_id.eq_ignore_ascii_case(TAP_HARDWARE_ID)
                || adapter
                    .hardware_id
                    .eq_ignore_ascii_case(TAP_HARDWARE_ID_WITH_ROOT))
    }) {
        remove_adapter_best_effort(tapctl, name).await;
        return Err(AppError::OpenVpnStartFailed {
            reason: "tapctl returned invalid data for the new TAP adapter".to_owned(),
        });
    }
    Ok(())
}

fn reserve_available(adapters: &[TapAdapterInfo]) -> Result<Option<String>, AppError> {
    let mut reserved = reserved_adapters().lock().map_err(|_| pool_lock_error())?;
    let available = adapters
        .iter()
        .find(|adapter| !reserved.contains(&adapter.name));
    let Some(adapter) = available else {
        return Ok(None);
    };
    reserved.insert(adapter.name.clone());
    Ok(Some(adapter.name.clone()))
}

fn reserve(name: &str) -> Result<(), AppError> {
    let mut reserved = reserved_adapters().lock().map_err(|_| pool_lock_error())?;
    reserved.insert(name.to_owned());
    Ok(())
}

fn next_pool_name(adapters: &[TapAdapterInfo]) -> Option<String> {
    let existing = adapters
        .iter()
        .filter_map(|adapter| pool_index(&adapter.name))
        .collect::<HashSet<_>>();
    (1..=MAX_POOL_SIZE)
        .find(|index| !existing.contains(index))
        .map(|index| format!("{ADAPTER_NAME_PREFIX}{index}"))
}

fn pool_index(name: &str) -> Option<usize> {
    let index = name.strip_prefix(ADAPTER_NAME_PREFIX)?.parse().ok()?;
    (1..=MAX_POOL_SIZE).contains(&index).then_some(index)
}

fn parse_adapter_line(line: &str) -> Option<TapAdapterInfo> {
    let mut fields = line.split('\t');
    let identifier = fields.next()?.trim();
    let name = fields.next()?.trim();
    let hardware_id = fields.next()?.trim();
    if fields.next().is_some() || name.is_empty() || hardware_id.is_empty() {
        return None;
    }
    let value = identifier.strip_prefix('{')?.strip_suffix('}')?;
    Uuid::parse_str(value).ok()?;
    Some(TapAdapterInfo {
        identifier: identifier.to_owned(),
        name: name.to_owned(),
        hardware_id: hardware_id.to_owned(),
    })
}

fn tapctl_path(openvpn: &Path) -> Result<PathBuf, AppError> {
    let path = openvpn
        .parent()
        .ok_or_else(|| AppError::OpenVpnInvalidExecutable {
            reason: "OpenVPN executable has no parent directory".to_owned(),
        })?
        .join(TAPCTL_EXECUTABLE);
    let metadata = fs::metadata(&path).map_err(|_| AppError::OpenVpnStartFailed {
        reason: "tapctl.exe was not found beside openvpn.exe".to_owned(),
    })?;
    if !metadata.is_file() {
        return Err(AppError::OpenVpnStartFailed {
            reason: "tapctl.exe is not a regular file".to_owned(),
        });
    }
    Ok(path)
}

fn tapctl_command(tapctl: &Path) -> Command {
    let mut command = Command::new(tapctl);
    command.creation_flags(CREATE_NO_WINDOW).kill_on_drop(true);
    command
}

async fn run_tapctl(
    mut command: Command,
    operation: &str,
) -> Result<std::process::Output, AppError> {
    timeout(TAPCTL_TIMEOUT, command.output())
        .await
        .map_err(|_| AppError::OpenVpnStartFailed {
            reason: format!("tapctl timed out while trying to {operation}"),
        })?
        .map_err(|error| adapter_start_error(operation, error))
}

async fn remove_adapter_best_effort(tapctl: &Path, identifier: &str) {
    let mut command = tapctl_command(tapctl);
    command.args(["delete", identifier]);
    let _ = timeout(TAPCTL_TIMEOUT, command.status()).await;
}

fn allocation_lock() -> &'static AsyncMutex<()> {
    ALLOCATION_LOCK.get_or_init(|| AsyncMutex::new(()))
}

fn reserved_adapters() -> &'static Mutex<HashSet<String>> {
    RESERVED_ADAPTERS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn adapter_start_error(operation: &str, error: std::io::Error) -> AppError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        AppError::PermissionDenied {
            operation: format!("trying to {operation}"),
        }
    } else {
        AppError::OpenVpnStartFailed {
            reason: format!("tapctl failed while trying to {operation}: {error}"),
        }
    }
}

fn pool_lock_error() -> AppError {
    AppError::OpenVpnStartFailed {
        reason: "Windows TAP adapter pool lock is poisoned".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{next_pool_name, parse_adapter_line, pool_index, TapAdapterInfo};

    fn adapter(index: usize) -> TapAdapterInfo {
        TapAdapterInfo {
            identifier: format!("{{DB489216-7804-4BFF-823D-E74D82C101D{index}}}"),
            name: format!("RoutePilot TAP {index}"),
            hardware_id: "root\\tap0901".to_owned(),
        }
    }

    #[test]
    fn parses_tapctl_output() {
        let parsed = parse_adapter_line(
            "{DB489216-7804-4BFF-823D-E74D82C101D0}\tRoutePilot TAP 1\troot\\tap0901",
        )
        .expect("adapter output should parse");

        assert_eq!(parsed.name, "RoutePilot TAP 1");
        assert_eq!(parsed.hardware_id, "root\\tap0901");
    }

    #[test]
    fn rejects_invalid_tapctl_output_and_pool_names() {
        assert!(parse_adapter_line("not-a-guid\tRoutePilot TAP 1\ttap0901").is_none());
        assert!(parse_adapter_line("").is_none());
        assert_eq!(pool_index("RoutePilot TAP 1"), Some(1));
        assert_eq!(pool_index("RoutePilot TAP 0"), None);
        assert_eq!(pool_index("Other TAP 1"), None);
    }

    #[test]
    fn fills_the_first_gap_in_the_persistent_pool() {
        assert_eq!(
            next_pool_name(&[adapter(1), adapter(3)]).as_deref(),
            Some("RoutePilot TAP 2")
        );
    }
}
