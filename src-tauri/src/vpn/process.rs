use std::{fs, io, path::Path, process::ExitStatus, time::Duration};

#[cfg(any(not(target_os = "macos"), test))]
use std::{ffi::OsString, path::PathBuf, process::Stdio};

use tokio::{process::Child, sync::broadcast, task::JoinHandle, time::timeout};

#[cfg(any(not(target_os = "macos"), test))]
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
};

use crate::{domain::ProfileId, error::AppError};

use super::locator::resolved_executable;
#[cfg(any(not(target_os = "macos"), test))]
use super::management::MANAGEMENT_HOST;

const OUTPUT_CHANNEL_CAPACITY: usize = 512;
#[cfg(any(not(target_os = "macos"), test))]
const MAX_OUTPUT_CHARACTERS: usize = 16 * 1024;
const OUTPUT_TASK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
#[cfg(any(not(target_os = "macos"), test))]
const REDACTED_OUTPUT: &str = "[sensitive OpenVPN output redacted]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessOutputStream {
    Stdout,
    Stderr,
}

/// A single sanitized line captured from the OpenVPN child process.
///
/// This payload intentionally does not implement `Debug` or serialization so a
/// caller cannot accidentally write connection output to application logs.
#[derive(Clone)]
pub struct ProcessOutput {
    pub profile_id: ProfileId,
    pub stream: ProcessOutputStream,
    pub line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExit {
    pub code: Option<i32>,
    pub success: bool,
}

impl From<ExitStatus> for ProcessExit {
    fn from(status: ExitStatus) -> Self {
        Self {
            code: status.code(),
            success: status.success(),
        }
    }
}

/// Process launch options reserved for the OpenVPN Management Interface.
/// The host is intentionally fixed to loopback and cannot be user-controlled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenVpnManagementOptions {
    port: u16,
}

impl OpenVpnManagementOptions {
    pub fn new(port: u16) -> Result<Self, AppError> {
        if port == 0 {
            return Err(AppError::ConfigInvalid {
                reason: "management port cannot be zero".to_owned(),
            });
        }
        Ok(Self { port })
    }

    #[must_use]
    pub const fn port(self) -> u16 {
        self.port
    }
}

/// Validated, immutable inputs for launching exactly one OpenVPN process.
pub struct OpenVpnLaunchConfig {
    profile_id: ProfileId,
    #[cfg(any(not(target_os = "macos"), test))]
    #[cfg_attr(all(target_os = "macos", test), allow(dead_code))]
    executable: PathBuf,
    #[cfg(any(not(target_os = "macos"), test))]
    config_path: PathBuf,
    #[cfg(any(not(target_os = "macos"), test))]
    #[cfg_attr(all(target_os = "macos", test), allow(dead_code))]
    working_directory: PathBuf,
    management: Option<OpenVpnManagementOptions>,
}

impl OpenVpnLaunchConfig {
    pub fn new(
        profile_id: ProfileId,
        executable: &Path,
        config_path: &Path,
    ) -> Result<Self, AppError> {
        let executable =
            resolved_executable(executable).ok_or_else(|| AppError::OpenVpnInvalidExecutable {
                reason: "OpenVPN executable disappeared or is no longer executable".to_owned(),
            })?;
        let config_path = fs::canonicalize(config_path).map_err(|_| AppError::ConfigInvalid {
            reason: "OpenVPN configuration does not exist".to_owned(),
        })?;
        let config_metadata = fs::metadata(&config_path)?;
        if !config_metadata.is_file() {
            return Err(AppError::ConfigInvalid {
                reason: "OpenVPN configuration is not a regular file".to_owned(),
            });
        }
        let working_directory = config_path
            .parent()
            .ok_or_else(|| AppError::ConfigInvalid {
                reason: "OpenVPN configuration has no parent directory".to_owned(),
            })?
            .to_path_buf();

        #[cfg(all(target_os = "macos", not(test)))]
        {
            let _ = (&executable, &working_directory);
        }

        Ok(Self {
            profile_id,
            #[cfg(any(not(target_os = "macos"), test))]
            executable,
            #[cfg(any(not(target_os = "macos"), test))]
            config_path,
            #[cfg(any(not(target_os = "macos"), test))]
            working_directory,
            management: None,
        })
    }

    #[must_use]
    pub fn with_management(mut self, management: OpenVpnManagementOptions) -> Self {
        self.management = Some(management);
        self
    }

    #[must_use]
    pub fn profile_id(&self) -> &ProfileId {
        &self.profile_id
    }
}

/// Handle for one child OpenVPN process.
///
/// Multi-profile ownership is intentionally left to the VPN manager introduced
/// in Milestone 7.
pub struct OpenVpnProcess {
    profile_id: ProfileId,
    process_id: Option<u32>,
    child: Option<Child>,
    #[cfg(target_os = "macos")]
    helper: Option<super::privileged_helper::HelperProcessClient>,
    output_sender: broadcast::Sender<ProcessOutput>,
    initial_output_receiver: Option<broadcast::Receiver<ProcessOutput>>,
    stdout_task: Option<JoinHandle<()>>,
    stderr_task: Option<JoinHandle<()>>,
    exit: Option<ProcessExit>,
}

impl OpenVpnProcess {
    pub async fn start(config: OpenVpnLaunchConfig) -> Result<Self, AppError> {
        #[cfg(target_os = "macos")]
        {
            let management = config
                .management
                .ok_or_else(|| AppError::OpenVpnStartFailed {
                    reason: "macOS helper launch requires a Management Interface port".to_owned(),
                })?;
            let (helper, process_id) = super::privileged_helper::HelperProcessClient::start(
                &config.profile_id,
                management.port(),
            )?;
            let (output_sender, output_receiver) = broadcast::channel(OUTPUT_CHANNEL_CAPACITY);
            Ok(Self {
                profile_id: config.profile_id,
                process_id: Some(process_id),
                child: None,
                helper: Some(helper),
                output_sender,
                initial_output_receiver: Some(output_receiver),
                stdout_task: None,
                stderr_task: None,
                exit: None,
            })
        }

        #[cfg(not(target_os = "macos"))]
        {
            let profile_id = config.profile_id.clone();
            let command = build_openvpn_command(&config)?;
            Self::spawn_command(profile_id, command).await
        }
    }

    #[must_use]
    pub fn profile_id(&self) -> &ProfileId {
        &self.profile_id
    }

    #[must_use]
    pub const fn process_id(&self) -> Option<u32> {
        self.process_id
    }

    #[must_use]
    pub const fn exit_status(&self) -> Option<ProcessExit> {
        self.exit
    }

    /// Returns the initial receiver once so startup output is not lost. Later
    /// calls subscribe to future output from the same process.
    pub fn take_output_receiver(&mut self) -> broadcast::Receiver<ProcessOutput> {
        self.initial_output_receiver
            .take()
            .unwrap_or_else(|| self.output_sender.subscribe())
    }

    pub fn subscribe_output(&self) -> broadcast::Receiver<ProcessOutput> {
        self.output_sender.subscribe()
    }

    pub fn is_running(&mut self) -> Result<bool, AppError> {
        if self.exit.is_some() {
            return Ok(false);
        }

        #[cfg(target_os = "macos")]
        if let Some(helper) = self.helper.as_mut() {
            let state = helper.status()?;
            if !state.running {
                self.exit = Some(ProcessExit {
                    code: state.exit_code,
                    success: state.exit_code == Some(0),
                });
            }
            return Ok(state.running);
        }

        let child = self
            .child
            .as_mut()
            .ok_or_else(|| AppError::OpenVpnStartFailed {
                reason: "OpenVPN process handle is missing".to_owned(),
            })?;
        match child.try_wait()? {
            Some(status) => {
                self.exit = Some(status.into());
                Ok(false)
            }
            None => Ok(true),
        }
    }

    pub async fn stop(&mut self) -> Result<ProcessExit, AppError> {
        if let Some(exit) = self.exit {
            self.finish_output_tasks().await;
            return Ok(exit);
        }

        #[cfg(target_os = "macos")]
        if let Some(mut helper) = self.helper.take() {
            let state = helper.stop().map_err(|error| AppError::OpenVpnStopFailed {
                reason: error.to_string(),
            })?;
            let exit = ProcessExit {
                code: state.exit_code,
                // A requested SIGTERM has no numeric exit code on Unix but is a
                // successful RoutePilot stop operation.
                success: true,
            };
            self.exit = Some(exit);
            self.process_id = None;
            self.finish_output_tasks().await;
            return Ok(exit);
        }

        let child = self
            .child
            .as_mut()
            .ok_or_else(|| AppError::OpenVpnStopFailed {
                reason: "OpenVPN process handle is missing".to_owned(),
            })?;
        let status = match child.try_wait().map_err(stop_error)? {
            Some(status) => status,
            None => {
                if let Err(kill_error) = child.start_kill() {
                    match child.try_wait().map_err(stop_error)? {
                        Some(status) => status,
                        None => return Err(stop_error(kill_error)),
                    }
                } else {
                    child.wait().await.map_err(stop_error)?
                }
            }
        };

        let exit = ProcessExit::from(status);
        self.exit = Some(exit);
        self.finish_output_tasks().await;
        Ok(exit)
    }

    #[cfg(any(not(target_os = "macos"), test))]
    pub(super) async fn spawn_command(
        profile_id: ProfileId,
        command: Command,
    ) -> Result<Self, AppError> {
        let mut command = command;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => return Err(start_error(error)),
        };
        let process_id = child.id();
        let Some(stdout) = child.stdout.take() else {
            let _ = child.start_kill();
            return Err(AppError::OpenVpnStartFailed {
                reason: "failed to capture OpenVPN stdout".to_owned(),
            });
        };
        let Some(stderr) = child.stderr.take() else {
            let _ = child.start_kill();
            return Err(AppError::OpenVpnStartFailed {
                reason: "failed to capture OpenVPN stderr".to_owned(),
            });
        };

        let (output_sender, output_receiver) = broadcast::channel(OUTPUT_CHANNEL_CAPACITY);
        let stdout_task = tokio::spawn(read_output(
            stdout,
            profile_id.clone(),
            ProcessOutputStream::Stdout,
            output_sender.clone(),
        ));
        let stderr_task = tokio::spawn(read_output(
            stderr,
            profile_id.clone(),
            ProcessOutputStream::Stderr,
            output_sender.clone(),
        ));

        Ok(Self {
            profile_id,
            process_id,
            child: Some(child),
            #[cfg(target_os = "macos")]
            helper: None,
            output_sender,
            initial_output_receiver: Some(output_receiver),
            stdout_task: Some(stdout_task),
            stderr_task: Some(stderr_task),
            exit: None,
        })
    }

    async fn finish_output_tasks(&mut self) {
        finish_output_task(self.stdout_task.take()).await;
        finish_output_task(self.stderr_task.take()).await;
    }
}

impl Drop for OpenVpnProcess {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        self.helper.take();
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
        if let Some(task) = self.stdout_task.take() {
            task.abort();
        }
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }
    }
}

#[cfg(any(not(target_os = "macos"), test))]
fn openvpn_arguments(config: &OpenVpnLaunchConfig) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("--config"),
        config.config_path.clone().into(),
    ];

    if let Some(management) = config.management {
        arguments.extend([
            OsString::from("--management"),
            OsString::from(MANAGEMENT_HOST),
            OsString::from(management.port().to_string()),
            OsString::from("--management-client"),
            OsString::from("--management-query-passwords"),
        ]);
    }

    arguments
}

#[cfg(not(target_os = "macos"))]
fn build_openvpn_command(config: &OpenVpnLaunchConfig) -> Result<Command, AppError> {
    let mut command = Command::new(&config.executable);
    command
        .args(openvpn_arguments(config))
        .current_dir(&config.working_directory);

    Ok(command)
}

#[cfg(any(not(target_os = "macos"), test))]
async fn read_output<R>(
    output: R,
    profile_id: ProfileId,
    stream: ProcessOutputStream,
    sender: broadcast::Sender<ProcessOutput>,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(output).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let _ = sender.send(ProcessOutput {
            profile_id: profile_id.clone(),
            stream,
            line: sanitize_output_line(&line),
        });
    }
}

#[cfg(any(not(target_os = "macos"), test))]
fn sanitize_output_line(line: &str) -> String {
    let lowercase = line.to_ascii_lowercase();
    if ["password", "token", "auth-user-pass"]
        .iter()
        .any(|sensitive_term| lowercase.contains(sensitive_term))
    {
        return REDACTED_OUTPUT.to_owned();
    }

    line.chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .take(MAX_OUTPUT_CHARACTERS)
        .collect()
}

async fn finish_output_task(task: Option<JoinHandle<()>>) {
    let Some(mut task) = task else {
        return;
    };

    if timeout(OUTPUT_TASK_SHUTDOWN_TIMEOUT, &mut task)
        .await
        .is_err()
    {
        task.abort();
    }
}

#[cfg(any(not(target_os = "macos"), test))]
fn start_error(error: io::Error) -> AppError {
    match error.kind() {
        io::ErrorKind::NotFound => AppError::OpenVpnNotFound,
        io::ErrorKind::PermissionDenied => AppError::PermissionDenied {
            operation: "starting OpenVPN".to_owned(),
        },
        _ => AppError::OpenVpnStartFailed {
            reason: error.to_string(),
        },
    }
}

fn stop_error(error: io::Error) -> AppError {
    AppError::OpenVpnStopFailed {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, time::Duration};

    use tempfile::TempDir;
    use tokio::{process::Command, time::timeout};

    use crate::domain::ProfileId;

    use super::{
        openvpn_arguments, OpenVpnLaunchConfig, OpenVpnManagementOptions, OpenVpnProcess,
        ProcessOutputStream, REDACTED_OUTPUT,
    };

    fn create_executable(path: &Path, contents: &str) {
        fs::write(path, contents).expect("test executable should be written");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .expect("test executable permissions should be set");
        }
    }

    #[test]
    fn builds_config_and_loopback_management_arguments() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let executable = workspace.path().join(if cfg!(windows) {
            "openvpn.exe"
        } else {
            "openvpn"
        });
        let config_path = workspace.path().join("config.ovpn");
        create_executable(&executable, "test executable");
        fs::write(&config_path, "client\n").expect("test config should be written");
        let management =
            OpenVpnManagementOptions::new(25_001).expect("management options should be valid");
        let config = OpenVpnLaunchConfig::new(
            ProfileId::new("vpn-a").expect("profile ID should be valid"),
            &executable,
            &config_path,
        )
        .expect("launch config should be valid")
        .with_management(management);

        let canonical_config =
            fs::canonicalize(&config_path).expect("config path should canonicalize");
        let arguments = openvpn_arguments(&config)
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(arguments[0], "--config");
        assert_eq!(Path::new(&arguments[1]), canonical_config);
        assert_eq!(arguments[2], "--management");
        assert_eq!(arguments[3], "127.0.0.1");
        assert_eq!(arguments[4], "25001");
        assert_eq!(arguments[5], "--management-client");
        assert_eq!(arguments[6], "--management-query-passwords");
    }

    #[test]
    fn rejects_zero_management_port_and_missing_config() {
        assert!(OpenVpnManagementOptions::new(0).is_err());

        let workspace = TempDir::new().expect("temporary directory should be created");
        let executable = workspace.path().join(if cfg!(windows) {
            "openvpn.exe"
        } else {
            "openvpn"
        });
        create_executable(&executable, "test executable");
        let result = OpenVpnLaunchConfig::new(
            ProfileId::new("vpn-a").expect("profile ID should be valid"),
            &executable,
            &workspace.path().join("missing.ovpn"),
        );

        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn starts_captures_sanitized_output_and_stops_process() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let executable = workspace.path().join("fake-openvpn");
        create_executable(
            &executable,
            "#!/bin/sh\nprintf 'connected\\n'\nprintf 'password=secret\\n' >&2\nwhile :; do sleep 1; done\n",
        );
        let profile_id = ProfileId::new("vpn-a").expect("profile ID should be valid");
        let command = Command::new(&executable);
        let mut process = OpenVpnProcess::spawn_command(profile_id.clone(), command)
            .await
            .expect("process should start");
        let mut output = process.take_output_receiver();

        assert_eq!(process.profile_id(), &profile_id);
        assert!(process.process_id().is_some());
        assert!(process.is_running().expect("status should be readable"));

        let mut lines = Vec::new();
        for _ in 0..2 {
            let message = timeout(Duration::from_secs(2), output.recv())
                .await
                .expect("output should arrive")
                .expect("output channel should remain open");
            assert_eq!(message.profile_id, profile_id);
            lines.push((message.stream, message.line));
        }

        assert!(lines.iter().any(|(stream, line)| {
            *stream == ProcessOutputStream::Stdout && line == "connected"
        }));
        assert!(lines.iter().any(|(stream, line)| {
            *stream == ProcessOutputStream::Stderr && line == REDACTED_OUTPUT
        }));

        let _ = process.stop().await.expect("process should stop");
        assert!(!process.is_running().expect("status should be readable"));
        assert!(process.exit_status().is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn observes_unexpected_process_exit() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let executable = workspace.path().join("fake-openvpn");
        create_executable(&executable, "#!/bin/sh\nexit 7\n");
        let mut process = OpenVpnProcess::spawn_command(
            ProfileId::new("vpn-a").expect("profile ID should be valid"),
            Command::new(&executable),
        )
        .await
        .expect("process should start");

        timeout(Duration::from_secs(2), async {
            while process.is_running().expect("status should be readable") {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("process should exit");

        assert_eq!(
            process.exit_status().and_then(|status| status.code),
            Some(7)
        );
        let exit = process
            .stop()
            .await
            .expect("stopping exited process should work");
        assert_eq!(exit.code, Some(7));
    }
}
