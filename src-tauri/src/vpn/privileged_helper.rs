//! macOS privileged OpenVPN helper and its local control protocol.
//!
//! The installed daemon is root-owned, but it never accepts an executable or
//! configuration path from the client. It derives the requesting user's
//! RoutePilot profile directory from the Unix peer credentials, validates the
//! imported client-only configuration again, and runs OpenVPN from a root-owned
//! snapshot for the lifetime of one control connection.

use std::{
    collections::{HashSet, VecDeque},
    ffi::{CStr, CString, OsStr},
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, RawFd},
        unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::UnixStream,
        },
    },
    path::{Component, Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{domain::ProfileId, error::AppError};

use super::parser::ParsedOvpnConfig;

pub const HELPER_LABEL: &str = "com.routepilot.client.helper";
pub const HELPER_VERSION: u32 = 5;
pub const HELPER_SOCKET_PATH: &str = "/var/run/com.routepilot.client.helper.sock";
pub const INSTALLED_HELPER_PATH: &str =
    "/Library/PrivilegedHelperTools/com.routepilot.client.helper";
pub const INSTALLED_RUNTIME_DIRECTORY: &str =
    "/Library/PrivilegedHelperTools/com.routepilot.client.runtime";
pub const INSTALLED_PLIST_PATH: &str = "/Library/LaunchDaemons/com.routepilot.client.helper.plist";

const SNAPSHOT_ROOT: &str = "/var/run/com.routepilot.client";
const OPENVPN_RELATIVE_PATH: &str = "openvpn";
const CONFIG_FILE: &str = "runtime.ovpn";
const MAX_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ASSET_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DIAGNOSTIC_LINES: usize = 32;
const MAX_DIAGNOSTIC_CHARACTERS: usize = 512;
const CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(3);
const CHILD_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONCURRENT_CLIENTS: usize = 64;

const INSTALLER_APPLE_SCRIPT: &str = r#"on run argv
    if (count of argv) is not 1 then error "RoutePilot helper installation command is missing"
    do shell script (item 1 of argv) with administrator privileges with prompt "RoutePilot needs administrator access once to enable secure VPN connections."
end run"#;

static ACTIVE_PROFILES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static ACTIVE_PIDS: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();
static ACTIVE_CLIENTS: AtomicUsize = AtomicUsize::new(0);

#[derive(Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum HelperRequest {
    Ping,
    Start {
        profile_id: String,
        management_port: u16,
    },
    Status,
    Stop,
}

#[derive(Serialize, Deserialize)]
struct HelperResponse {
    version: u32,
    #[serde(default)]
    management_client: bool,
    ok: bool,
    pid: Option<u32>,
    running: Option<bool>,
    exit_code: Option<i32>,
    error: Option<String>,
}

impl HelperResponse {
    fn ok() -> Self {
        Self {
            version: HELPER_VERSION,
            management_client: true,
            ok: true,
            pid: None,
            running: None,
            exit_code: None,
            error: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            version: HELPER_VERSION,
            management_client: true,
            ok: false,
            pid: None,
            running: None,
            exit_code: None,
            error: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelperProcessState {
    pub running: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HelperInstallationState {
    Installed,
    NotInstalled,
    Unavailable,
    Outdated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperInstallationStatus {
    pub state: HelperInstallationState,
    pub installed_version: Option<u32>,
    pub expected_version: u32,
}

#[must_use]
pub fn installation_status() -> HelperInstallationStatus {
    match helper_health() {
        Ok(response) if is_compatible(&response) => HelperInstallationStatus {
            state: HelperInstallationState::Installed,
            installed_version: Some(response.version),
            expected_version: HELPER_VERSION,
        },
        Ok(response) => HelperInstallationStatus {
            state: HelperInstallationState::Outdated,
            installed_version: Some(response.version),
            expected_version: HELPER_VERSION,
        },
        Err(_)
            if Path::new(INSTALLED_HELPER_PATH).exists()
                || Path::new(INSTALLED_PLIST_PATH).exists() =>
        {
            HelperInstallationStatus {
                state: HelperInstallationState::Unavailable,
                installed_version: None,
                expected_version: HELPER_VERSION,
            }
        }
        Err(_) => HelperInstallationStatus {
            state: HelperInstallationState::NotInstalled,
            installed_version: None,
            expected_version: HELPER_VERSION,
        },
    }
}

pub fn install(resource_directory: &Path) -> Result<HelperInstallationStatus, AppError> {
    let app_bundle = resource_directory
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| AppError::PrivilegedHelperInstallFailed {
            reason: "the RoutePilot application bundle could not be located".to_owned(),
        })?;
    let helper_source = resource_directory.join("routepilot-helper");
    let runtime_source = resource_directory.join("binaries").join("macos");
    let openvpn_source = runtime_source.join(OPENVPN_RELATIVE_PATH);
    let plist_source = resource_directory.join(format!("{HELPER_LABEL}.plist"));
    for (path, description) in [
        (&helper_source, "privileged helper"),
        (&openvpn_source, "bundled OpenVPN runtime"),
        (&plist_source, "launch daemon property list"),
    ] {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            AppError::PrivilegedHelperInstallFailed {
                reason: format!("{description} is missing: {error}"),
            }
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(AppError::PrivilegedHelperInstallFailed {
                reason: format!("{description} is not a regular bundled file"),
            });
        }
    }

    let staging_runtime = format!("{INSTALLED_RUNTIME_DIRECTORY}.new");
    let staging_helper = format!("{INSTALLED_HELPER_PATH}.new");
    let staging_plist = format!("{INSTALLED_PLIST_PATH}.new");
    let command = format!(
        "set -eu; umask 022; /usr/bin/codesign --verify --deep --strict {app_bundle}; service_loaded=0; if /bin/launchctl print system/{label} >/dev/null 2>&1; then service_loaded=1; fi; /bin/rm -f {socket}; /bin/rm -rf {staging_runtime}; /usr/bin/ditto {runtime_source} {staging_runtime}; /usr/sbin/chown -R root:wheel {staging_runtime}; /bin/chmod -R go-w {staging_runtime}; /bin/chmod 0755 {staging_runtime} {staging_runtime}/openvpn {staging_runtime}/lib; /bin/rm -rf {runtime}; /bin/mv {staging_runtime} {runtime}; /usr/bin/install -o root -g wheel -m 0755 {helper_source} {staging_helper}; /bin/mv -f {staging_helper} {helper}; /usr/bin/install -o root -g wheel -m 0644 {plist_source} {staging_plist}; /usr/bin/plutil -lint {staging_plist} >/dev/null; /bin/mv -f {staging_plist} {plist}; if [ \"$service_loaded\" -eq 0 ]; then /bin/launchctl bootstrap system {plist}; fi; /bin/launchctl kickstart -k system/{label} >/dev/null 2>&1 || true",
        app_bundle = shell_quote_path(app_bundle)?,
        label = HELPER_LABEL,
        socket = shell_quote_path(Path::new(HELPER_SOCKET_PATH))?,
        staging_runtime = shell_quote_path(Path::new(&staging_runtime))?,
        runtime_source = shell_quote_path(&runtime_source)?,
        runtime = shell_quote_path(Path::new(INSTALLED_RUNTIME_DIRECTORY))?,
        helper_source = shell_quote_path(&helper_source)?,
        staging_helper = shell_quote_path(Path::new(&staging_helper))?,
        helper = shell_quote_path(Path::new(INSTALLED_HELPER_PATH))?,
        plist_source = shell_quote_path(&plist_source)?,
        staging_plist = shell_quote_path(Path::new(&staging_plist))?,
        plist = shell_quote_path(Path::new(INSTALLED_PLIST_PATH))?,
    );

    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(INSTALLER_APPLE_SCRIPT)
        .arg(command)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| AppError::PrivilegedHelperInstallFailed {
            reason: error.to_string(),
        })?;
    if !output.status.success() {
        let cancelled = String::from_utf8_lossy(&output.stderr).contains("User canceled");
        return Err(if cancelled {
            AppError::PermissionDenied {
                operation: "enabling the RoutePilot system helper was cancelled".to_owned(),
            }
        } else {
            AppError::PrivilegedHelperInstallFailed {
                reason: "macOS rejected the helper installation".to_owned(),
            }
        });
    }

    // An ad-hoc local build can be held briefly by macOS background-item and
    // code-signature checks before launchd retries it. Keep the UI action alive
    // long enough for that retry instead of incorrectly reporting a failure.
    for _ in 0..300 {
        let status = installation_status();
        if status.state == HelperInstallationState::Installed {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(AppError::PrivilegedHelperInstallFailed {
        reason: "the helper did not become ready after installation".to_owned(),
    })
}

fn shell_quote_path(path: &Path) -> Result<String, AppError> {
    let path = path
        .to_str()
        .ok_or_else(|| AppError::PrivilegedHelperInstallFailed {
            reason: "helper installation paths must be valid UTF-8".to_owned(),
        })?;
    Ok(format!("'{}'", path.replace('\'', "'\\''")))
}

/// A live control connection. Dropping it closes the session, which makes the
/// daemon terminate the associated OpenVPN process.
pub struct HelperProcessClient {
    stream: UnixStream,
}

impl HelperProcessClient {
    pub fn start(profile_id: &ProfileId, management_port: u16) -> Result<(Self, u32), AppError> {
        if management_port == 0 {
            return Err(AppError::ConfigInvalid {
                reason: "management port cannot be zero".to_owned(),
            });
        }
        validate_compatibility(&helper_health()?)?;

        let mut stream = connect_socket()?;
        send_request(
            &mut stream,
            &HelperRequest::Start {
                profile_id: profile_id.to_string(),
                management_port,
            },
        )?;
        let response = read_response(&mut stream)?;
        validate_compatibility(&response)?;
        if !response.ok {
            return Err(AppError::OpenVpnStartFailed {
                reason: response
                    .error
                    .unwrap_or_else(|| "the helper rejected the start request".to_owned()),
            });
        }
        let pid = response
            .pid
            .ok_or_else(|| AppError::PrivilegedHelperUnavailable {
                reason: "helper did not return an OpenVPN process ID".to_owned(),
            })?;

        Ok((Self { stream }, pid))
    }

    pub fn status(&mut self) -> Result<HelperProcessState, AppError> {
        send_request(&mut self.stream, &HelperRequest::Status)?;
        let response = read_response(&mut self.stream)?;
        validate_response(&response)?;
        if response.running == Some(false) {
            if let Some(reason) = response.error {
                return Err(AppError::OpenVpnStartFailed { reason });
            }
        }
        Ok(HelperProcessState {
            running: response.running.unwrap_or(false),
            exit_code: response.exit_code,
        })
    }

    pub fn stop(&mut self) -> Result<HelperProcessState, AppError> {
        send_request(&mut self.stream, &HelperRequest::Stop)?;
        let response = read_response(&mut self.stream)?;
        validate_response(&response)?;
        Ok(HelperProcessState {
            running: response.running.unwrap_or(false),
            exit_code: response.exit_code,
        })
    }
}

impl Drop for HelperProcessClient {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

fn helper_health() -> Result<HelperResponse, AppError> {
    let mut stream = connect_socket()?;
    send_request(&mut stream, &HelperRequest::Ping)?;
    let response = read_response(&mut stream)?;
    if !response.ok {
        return Err(AppError::PrivilegedHelperUnavailable {
            reason: response
                .error
                .clone()
                .unwrap_or_else(|| "helper rejected the health check".to_owned()),
        });
    }
    Ok(response)
}

fn connect_socket() -> Result<UnixStream, AppError> {
    let stream = UnixStream::connect(HELPER_SOCKET_PATH).map_err(|error| {
        AppError::PrivilegedHelperUnavailable {
            reason: error.to_string(),
        }
    })?;
    stream
        .set_read_timeout(Some(CLIENT_IO_TIMEOUT))
        .map_err(helper_io_error)?;
    stream
        .set_write_timeout(Some(CLIENT_IO_TIMEOUT))
        .map_err(helper_io_error)?;
    Ok(stream)
}

fn send_request(stream: &mut UnixStream, request: &HelperRequest) -> Result<(), AppError> {
    serde_json::to_writer(&mut *stream, request).map_err(|error| {
        AppError::PrivilegedHelperUnavailable {
            reason: error.to_string(),
        }
    })?;
    stream.write_all(b"\n").map_err(helper_io_error)?;
    stream.flush().map_err(helper_io_error)
}

fn read_response(stream: &mut UnixStream) -> Result<HelperResponse, AppError> {
    let line = read_line(stream).map_err(helper_io_error)?;
    serde_json::from_slice(&line).map_err(|error| AppError::PrivilegedHelperUnavailable {
        reason: format!("helper returned an invalid response: {error}"),
    })
}

fn validate_response(response: &HelperResponse) -> Result<(), AppError> {
    validate_compatibility(response)?;
    if !response.ok {
        return Err(AppError::PrivilegedHelperUnavailable {
            reason: response
                .error
                .clone()
                .unwrap_or_else(|| "helper rejected the request".to_owned()),
        });
    }
    Ok(())
}

fn validate_compatibility(response: &HelperResponse) -> Result<(), AppError> {
    if !is_compatible(response) {
        return Err(AppError::PrivilegedHelperUnavailable {
            reason: "installed helper is out of date".to_owned(),
        });
    }
    Ok(())
}

fn is_compatible(response: &HelperResponse) -> bool {
    response.version == HELPER_VERSION && response.management_client
}

fn helper_io_error(error: io::Error) -> AppError {
    AppError::PrivilegedHelperUnavailable {
        reason: error.to_string(),
    }
}

/// Entry point used by the bundled `routepilot-helper` executable.
pub fn run_daemon() -> Result<(), String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("the RoutePilot helper must run as root".to_owned());
    }

    let terminate = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(libc::SIGTERM, Arc::clone(&terminate))
        .map_err(|error| error.to_string())?;
    signal_hook::flag::register(libc::SIGINT, Arc::clone(&terminate))
        .map_err(|error| error.to_string())?;

    cleanup_snapshots().map_err(|error| error.to_string())?;
    prepare_socket_path().map_err(|error| error.to_string())?;
    let listener = std::os::unix::net::UnixListener::bind(HELPER_SOCKET_PATH)
        .map_err(|error| error.to_string())?;
    fs::set_permissions(HELPER_SOCKET_PATH, fs::Permissions::from_mode(0o666))
        .map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;

    while !terminate.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let accepted = ACTIVE_CLIENTS
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
                        (active < MAX_CONCURRENT_CLIENTS).then_some(active + 1)
                    })
                    .is_ok();
                if !accepted {
                    drop(stream);
                    continue;
                }
                thread::spawn(move || {
                    let _client_guard = ActiveClientGuard;
                    let _ = handle_connection(stream);
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                stop_all_children();
                let _ = cleanup_snapshots();
                let _ = fs::remove_file(HELPER_SOCKET_PATH);
                return Err(error.to_string());
            }
        }
    }

    stop_all_children();
    let _ = cleanup_snapshots();
    let _ = fs::remove_file(HELPER_SOCKET_PATH);
    Ok(())
}

fn prepare_socket_path() -> io::Result<()> {
    match fs::symlink_metadata(HELPER_SOCKET_PATH) {
        Ok(metadata) => {
            if !metadata.file_type().is_socket() || metadata.uid() != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refusing to replace an untrusted helper socket path",
                ));
            }
            fs::remove_file(HELPER_SOCKET_PATH)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn handle_connection(mut stream: UnixStream) -> Result<(), String> {
    configure_accepted_stream(&stream).map_err(|error| error.to_string())?;
    let peer_uid = peer_uid(&stream).map_err(|error| error.to_string())?;
    let request = read_request(&mut stream).map_err(|error| error.to_string())?;

    match request {
        HelperRequest::Ping => write_response(&mut stream, &HelperResponse::ok()),
        HelperRequest::Start {
            profile_id,
            management_port,
        } => start_session(stream, peer_uid, &profile_id, management_port),
        HelperRequest::Status | HelperRequest::Stop => write_response(
            &mut stream,
            &HelperResponse::error("a session has not been started"),
        ),
    }
}

fn configure_accepted_stream(stream: &UnixStream) -> io::Result<()> {
    // On macOS an accepted socket can inherit O_NONBLOCK from its listener.
    // The daemon listener must be nonblocking so it can observe SIGTERM, but a
    // per-VPN control connection must block while idle. Leaving O_NONBLOCK set
    // either spins on EAGAIN or closes the session immediately.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(CLIENT_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT))
}

fn start_session(
    mut stream: UnixStream,
    peer_uid: u32,
    profile_id: &str,
    management_port: u16,
) -> Result<(), String> {
    if management_port == 0 {
        return write_response(
            &mut stream,
            &HelperResponse::error("invalid management port"),
        );
    }
    let profile_id = ProfileId::new(profile_id.to_owned()).map_err(|error| error.to_string())?;
    let active_key = format!("{peer_uid}:{}", profile_id.as_str());
    let _profile_guard = match ActiveProfileGuard::acquire(active_key) {
        Some(guard) => guard,
        None => {
            return write_response(
                &mut stream,
                &HelperResponse::error("this profile already has an active helper session"),
            )
        }
    };

    let snapshot = match prepare_snapshot(peer_uid, &profile_id) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return write_response(
                &mut stream,
                &HelperResponse::error(format!("configuration was rejected: {error}")),
            )
        }
    };
    let _snapshot_guard = SnapshotGuard(snapshot.clone());

    let mut process = match spawn_openvpn(&snapshot, management_port) {
        Ok(process) => process,
        Err(error) => {
            return write_response(
                &mut stream,
                &HelperResponse::error(format!("OpenVPN could not be started: {error}")),
            )
        }
    };
    let pid = process.child.id();
    let _pid_guard = ActivePidGuard::register(pid as i32);
    let mut response = HelperResponse::ok();
    response.pid = Some(pid);
    write_response(&mut stream, &response)?;
    stream
        // A live session uses a blocking control channel. The accepted socket
        // was explicitly returned to blocking mode above; clearing the startup
        // timeout leaves the thread asleep between app status requests, while
        // EOF still wakes it immediately if the app exits.
        .set_read_timeout(None)
        .map_err(|error| error.to_string())?;

    loop {
        match read_request(&mut stream) {
            Ok(HelperRequest::Status) => {
                let mut response = HelperResponse::ok();
                match process.child.try_wait() {
                    Ok(Some(status)) => {
                        process.finish_output_tasks();
                        response.running = Some(false);
                        response.exit_code = status.code();
                        response.error = process.diagnostic(status);
                        write_response(&mut stream, &response)?;
                        return Ok(());
                    }
                    Ok(None) => {
                        response.running = Some(true);
                        write_response(&mut stream, &response)?;
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            Ok(HelperRequest::Stop) => {
                let status = match process.child.try_wait() {
                    Ok(Some(status)) => status,
                    Ok(None) => terminate_child(&mut process.child)?,
                    Err(error) => return Err(error.to_string()),
                };
                process.finish_output_tasks();
                let mut response = HelperResponse::ok();
                response.running = Some(false);
                response.exit_code = status.code();
                write_response(&mut stream, &response)?;
                return Ok(());
            }
            Ok(HelperRequest::Ping) => write_response(&mut stream, &HelperResponse::ok())?,
            Ok(HelperRequest::Start { .. }) => write_response(
                &mut stream,
                &HelperResponse::error("this control connection already owns a session"),
            )?,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                if process.child.try_wait().ok().flatten().is_none() {
                    let _ = terminate_child(&mut process.child);
                }
                process.finish_output_tasks();
                return Ok(());
            }
            Err(error) => {
                if process.child.try_wait().ok().flatten().is_none() {
                    let _ = terminate_child(&mut process.child);
                }
                process.finish_output_tasks();
                return Err(error.to_string());
            }
        }
    }
}

fn read_request(stream: &mut UnixStream) -> io::Result<HelperRequest> {
    let line = read_line(stream)?;
    serde_json::from_slice(&line)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

fn write_response(stream: &mut UnixStream, response: &HelperResponse) -> Result<(), String> {
    serde_json::to_writer(&mut *stream, response).map_err(|error| error.to_string())?;
    stream.write_all(b"\n").map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())
}

fn read_line(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(_) if byte[0] == b'\n' => return Ok(output),
            Ok(_) => {
                output.push(byte[0]);
                if output.len() > MAX_MESSAGE_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "helper message exceeds the size limit",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let result = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if result == 0 {
        Ok(uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

fn prepare_snapshot(peer_uid: u32, profile_id: &ProfileId) -> Result<PathBuf, String> {
    let home = home_directory(peer_uid).map_err(|error| error.to_string())?;
    let profile_directory = home
        .join("Library")
        .join("Application Support")
        .join("com.routepilot.client")
        .join("profiles")
        .join(profile_id.as_str());
    let directory =
        open_owned_directory(&profile_directory, peer_uid).map_err(|error| error.to_string())?;
    let config_bytes = read_owned_file_at(
        directory.as_raw_fd(),
        OsStr::new(CONFIG_FILE),
        peer_uid,
        MAX_CONFIG_BYTES,
    )
    .map_err(|error| error.to_string())?;
    let config_source = std::str::from_utf8(&config_bytes)
        .map_err(|_| "runtime configuration is not UTF-8".to_owned())?;
    let parsed = ParsedOvpnConfig::parse(config_source).map_err(|error| error.to_string())?;
    parsed
        .validate_privileged_client_config()
        .map_err(|error| error.to_string())?;

    let snapshot_root = Path::new(SNAPSHOT_ROOT);
    ensure_root_directory(snapshot_root).map_err(|error| error.to_string())?;
    let snapshot = snapshot_root.join(format!(
        "{}-{}-{}",
        peer_uid,
        profile_id.as_str(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&snapshot).map_err(|error| error.to_string())?;
    fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;

    let copy_result = (|| -> Result<(), String> {
        write_root_file(&snapshot.join(CONFIG_FILE), &config_bytes)
            .map_err(|error| error.to_string())?;
        let mut copied = HashSet::new();
        for reference in parsed.external_files() {
            let name = single_file_name(&reference.path).ok_or_else(|| {
                format!(
                    "{} reference at line {} must stay inside the profile directory",
                    reference.directive, reference.line_number
                )
            })?;
            if !copied.insert(name.to_os_string()) {
                continue;
            }
            let bytes = read_owned_file_at(directory.as_raw_fd(), name, peer_uid, MAX_ASSET_BYTES)
                .map_err(|error| error.to_string())?;
            write_root_file(&snapshot.join(name), &bytes).map_err(|error| error.to_string())?;
        }
        Ok(())
    })();

    if let Err(error) = copy_result {
        let _ = fs::remove_dir_all(&snapshot);
        return Err(error);
    }
    Ok(snapshot)
}

fn single_file_name(path: &Path) -> Option<&OsStr> {
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(name)), None) => Some(name),
        _ => None,
    }
}

fn home_directory(uid: u32) -> io::Result<PathBuf> {
    let buffer_size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let buffer_size = if buffer_size > 0 {
        buffer_size as usize
    } else {
        16 * 1024
    };
    let mut buffer = vec![0_i8; buffer_size];
    let mut password: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let code = unsafe {
        libc::getpwuid_r(
            uid,
            &mut password,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code));
    }
    if result.is_null() || password.pw_dir.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "requesting user has no home directory",
        ));
    }
    let bytes = unsafe { CStr::from_ptr(password.pw_dir) }.to_bytes();
    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

fn open_owned_directory(path: &Path, uid: u32) -> io::Result<File> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid profile path"))?;
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "profile directory ownership or permissions are unsafe",
        ));
    }
    Ok(file)
}

fn read_owned_file_at(
    directory_fd: RawFd,
    name: &OsStr,
    uid: u32,
    maximum_bytes: u64,
) -> io::Result<Vec<u8>> {
    let name = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid profile file name"))?;
    let fd = unsafe {
        libc::openat(
            directory_fd,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.mode() & 0o022 != 0
        || metadata.len() > maximum_bytes
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "profile file ownership, permissions, or size are unsafe",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn ensure_root_directory(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || metadata.uid() != 0
                || metadata.mode() & 0o022 != 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "helper snapshot directory is unsafe",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

fn cleanup_snapshots() -> io::Result<()> {
    let root = Path::new(SNAPSHOT_ROOT);
    ensure_root_directory(root)?;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn write_root_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

struct ManagedOpenVpn {
    child: Child,
    diagnostics: Arc<Mutex<VecDeque<String>>>,
    output_tasks: Vec<JoinHandle<()>>,
}

impl ManagedOpenVpn {
    fn finish_output_tasks(&mut self) {
        for task in self.output_tasks.drain(..) {
            let _ = task.join();
        }
    }

    fn diagnostic(&self, status: ExitStatus) -> Option<String> {
        self.diagnostics
            .lock()
            .ok()
            .and_then(|lines| lines.back().cloned())
            .or_else(|| {
                Some(format!(
                    "OpenVPN exited before startup completed ({status})"
                ))
            })
    }
}

fn spawn_openvpn(snapshot: &Path, management_port: u16) -> io::Result<ManagedOpenVpn> {
    let openvpn = Path::new(INSTALLED_RUNTIME_DIRECTORY).join(OPENVPN_RELATIVE_PATH);
    validate_root_runtime(&openvpn)?;
    let config_path = snapshot.join(CONFIG_FILE);
    let management_port = management_port.to_string();
    let mut child = Command::new(openvpn)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .args([
            OsStr::new("--config"),
            config_path.as_os_str(),
            OsStr::new("--management"),
            OsStr::new("127.0.0.1"),
            OsStr::new(&management_port),
            OsStr::new("--management-client"),
            OsStr::new("--management-query-passwords"),
            OsStr::new("--script-security"),
            OsStr::new("1"),
        ])
        .current_dir(snapshot)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let diagnostics = Arc::new(Mutex::new(VecDeque::with_capacity(MAX_DIAGNOSTIC_LINES)));
    let mut output_tasks = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        output_tasks.push(spawn_output_reader(stdout, Arc::clone(&diagnostics)));
    }
    if let Some(stderr) = child.stderr.take() {
        output_tasks.push(spawn_output_reader(stderr, Arc::clone(&diagnostics)));
    }
    Ok(ManagedOpenVpn {
        child,
        diagnostics,
        output_tasks,
    })
}

fn spawn_output_reader<R>(output: R, diagnostics: Arc<Mutex<VecDeque<String>>>) -> JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            let lowercase = line.to_ascii_lowercase();
            if ["password", "token", "auth-user-pass"]
                .iter()
                .any(|term| lowercase.contains(term))
            {
                continue;
            }
            let sanitized = line
                .chars()
                .filter(|character| !character.is_control() || *character == '\t')
                .take(MAX_DIAGNOSTIC_CHARACTERS)
                .collect::<String>();
            if let Ok(mut lines) = diagnostics.lock() {
                if lines.len() == MAX_DIAGNOSTIC_LINES {
                    lines.pop_front();
                }
                lines.push_back(sanitized);
            }
        }
    })
}

fn validate_root_runtime(openvpn: &Path) -> io::Result<()> {
    validate_root_directory(Path::new(INSTALLED_RUNTIME_DIRECTORY))?;
    let library_directory = Path::new(INSTALLED_RUNTIME_DIRECTORY).join("lib");
    validate_root_directory(&library_directory)?;
    for library in [
        "liblzo2.2.dylib",
        "liblz4.1.dylib",
        "libpkcs11-helper.1.dylib",
        "libssl.3.dylib",
        "libcrypto.3.dylib",
    ] {
        validate_root_file(&library_directory.join(library), false)?;
    }
    validate_root_file(openvpn, true)
}

fn validate_root_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "installed OpenVPN runtime directory is not root-owned and immutable",
        ));
    }
    Ok(())
}

fn validate_root_file(path: &Path, executable: bool) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || (executable && metadata.mode() & 0o111 == 0)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "installed OpenVPN runtime file is not root-owned and immutable",
        ));
    }
    Ok(())
}

fn terminate_child(child: &mut Child) -> Result<ExitStatus, String> {
    let pid = child.id() as i32;
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    let deadline = Instant::now() + CHILD_SHUTDOWN_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                child.kill().map_err(|error| error.to_string())?;
                return child.wait().map_err(|error| error.to_string());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn stop_all_children() {
    let pids = active_pids()
        .lock()
        .map(|pids| pids.iter().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    for pid in pids {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    thread::sleep(Duration::from_millis(500));
    let pids = active_pids()
        .lock()
        .map(|pids| pids.iter().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    for pid in pids {
        if unsafe { libc::kill(pid, 0) } == 0 {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

fn active_profiles() -> &'static Mutex<HashSet<String>> {
    ACTIVE_PROFILES.get_or_init(|| Mutex::new(HashSet::new()))
}

fn active_pids() -> &'static Mutex<HashSet<i32>> {
    ACTIVE_PIDS.get_or_init(|| Mutex::new(HashSet::new()))
}

struct ActiveProfileGuard(String);

struct ActiveClientGuard;

impl Drop for ActiveClientGuard {
    fn drop(&mut self) {
        ACTIVE_CLIENTS.fetch_sub(1, Ordering::Relaxed);
    }
}

impl ActiveProfileGuard {
    fn acquire(key: String) -> Option<Self> {
        let mut active = active_profiles().lock().ok()?;
        if !active.insert(key.clone()) {
            return None;
        }
        Some(Self(key))
    }
}

impl Drop for ActiveProfileGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = active_profiles().lock() {
            active.remove(&self.0);
        }
    }
}

struct ActivePidGuard(i32);

impl ActivePidGuard {
    fn register(pid: i32) -> Self {
        if let Ok(mut active) = active_pids().lock() {
            active.insert(pid);
        }
        Self(pid)
    }
}

impl Drop for ActivePidGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = active_pids().lock() {
            active.remove(&self.0);
        }
    }
}

struct SnapshotGuard(PathBuf);

impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use std::{os::fd::AsRawFd, os::unix::net::UnixStream, path::Path};

    use super::{
        configure_accepted_stream, is_compatible, single_file_name, HelperResponse, HELPER_VERSION,
    };

    #[test]
    fn rejects_a_legacy_helper_without_management_client_capability() {
        let response: HelperResponse = serde_json::from_value(serde_json::json!({
            "version": HELPER_VERSION,
            "ok": true,
            "pid": null,
            "running": null,
            "exit_code": null,
            "error": null
        }))
        .expect("legacy response should remain readable");

        assert!(!response.management_client);
        assert!(!is_compatible(&response));
        assert!(is_compatible(&HelperResponse::ok()));
    }

    #[test]
    fn accepts_only_profile_local_asset_names() {
        assert_eq!(
            single_file_name(Path::new("client.key")),
            Some(std::ffi::OsStr::new("client.key"))
        );
        assert!(single_file_name(Path::new("../client.key")).is_none());
        assert!(single_file_name(Path::new("certs/client.crt")).is_none());
        assert!(single_file_name(Path::new("/tmp/client.key")).is_none());
    }

    #[test]
    fn accepted_control_stream_is_forced_back_to_blocking_mode() {
        let (stream, _peer) = UnixStream::pair().expect("Unix stream pair should open");
        stream
            .set_nonblocking(true)
            .expect("test stream should become nonblocking");

        configure_accepted_stream(&stream).expect("accepted stream should be configured");

        let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(flags & libc::O_NONBLOCK, 0);
    }
}
