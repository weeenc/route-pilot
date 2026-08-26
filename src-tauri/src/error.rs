use std::io;

use serde::Serialize;
use thiserror::Error;

/// Errors produced by RoutePilot application and infrastructure services.
///
/// This type is intentionally not serializable. IPC commands must convert it to
/// [`ErrorPayload`] so Rust debug output and nested system errors never leak into
/// the UI by accident.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("VPN profile '{profile_id}' was not found")]
    ProfileNotFound { profile_id: String },

    #[error("OpenVPN configuration is invalid: {reason}")]
    ConfigInvalid { reason: String },

    #[error("OpenVPN executable was not found")]
    OpenVpnNotFound,

    #[error("OpenVPN executable is invalid: {reason}")]
    OpenVpnInvalidExecutable { reason: String },

    #[error("OpenVPN failed to start: {reason}")]
    OpenVpnStartFailed { reason: String },

    #[error("OpenVPN failed to stop: {reason}")]
    OpenVpnStopFailed { reason: String },

    #[error("VPN profile '{profile_id}' already has an active runtime")]
    ConnectionAlreadyActive { profile_id: String },

    #[error("OpenVPN Management Interface connection failed: {reason}")]
    ManagementConnectFailed { reason: String },

    #[error("OpenVPN Management Interface timed out")]
    ManagementTimeout,

    #[error("OpenVPN Management Interface protocol error: {reason}")]
    ManagementProtocolInvalid { reason: String },

    #[error("OpenVPN authentication failed")]
    AuthenticationFailed,

    #[error("permission denied while {operation}")]
    PermissionDenied { operation: String },

    #[error("the privileged helper is unavailable: {reason}")]
    PrivilegedHelperUnavailable { reason: String },

    #[error("the privileged helper could not be installed: {reason}")]
    PrivilegedHelperInstallFailed { reason: String },

    #[error("VPN routes conflict: {details}")]
    RouteConflict { details: String },

    #[error("profile storage is corrupted: {reason}")]
    ProfileStoreCorrupted { reason: String },

    #[error("application settings are corrupted: {reason}")]
    SettingsCorrupted { reason: String },

    #[error("I/O operation failed")]
    Io(#[from] io::Error),

    #[error("{feature} is not supported on this platform")]
    Unsupported { feature: String },
}

impl AppError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ProfileNotFound { .. } => "PROFILE_NOT_FOUND",
            Self::ConfigInvalid { .. } => "CONFIG_INVALID",
            Self::OpenVpnNotFound => "OPENVPN_NOT_FOUND",
            Self::OpenVpnInvalidExecutable { .. } => "OPENVPN_INVALID_EXECUTABLE",
            Self::OpenVpnStartFailed { .. } => "OPENVPN_START_FAILED",
            Self::OpenVpnStopFailed { .. } => "OPENVPN_STOP_FAILED",
            Self::ConnectionAlreadyActive { .. } => "CONNECTION_ALREADY_ACTIVE",
            Self::ManagementConnectFailed { .. } => "MANAGEMENT_CONNECT_FAILED",
            Self::ManagementTimeout => "MANAGEMENT_TIMEOUT",
            Self::ManagementProtocolInvalid { .. } => "MANAGEMENT_PROTOCOL_INVALID",
            Self::AuthenticationFailed => "AUTHENTICATION_FAILED",
            Self::PermissionDenied { .. } => "PERMISSION_DENIED",
            Self::PrivilegedHelperUnavailable { .. } => "PRIVILEGED_HELPER_UNAVAILABLE",
            Self::PrivilegedHelperInstallFailed { .. } => "PRIVILEGED_HELPER_INSTALL_FAILED",
            Self::RouteConflict { .. } => "ROUTE_CONFLICT",
            Self::ProfileStoreCorrupted { .. } => "PROFILE_STORE_CORRUPTED",
            Self::SettingsCorrupted { .. } => "SETTINGS_CORRUPTED",
            Self::Io(_) => "IO_ERROR",
            Self::Unsupported { .. } => "UNSUPPORTED",
        }
    }

    #[must_use]
    pub const fn public_message(&self) -> &'static str {
        match self {
            Self::ProfileNotFound { .. } => "VPN profile was not found",
            Self::ConfigInvalid { .. } => "OpenVPN configuration is invalid",
            Self::OpenVpnNotFound => "OpenVPN executable was not found",
            Self::OpenVpnInvalidExecutable { .. } => "OpenVPN executable is not valid",
            Self::OpenVpnStartFailed { .. } => "Failed to start OpenVPN",
            Self::OpenVpnStopFailed { .. } => "Failed to stop OpenVPN",
            Self::ConnectionAlreadyActive { .. } => {
                "This VPN profile already has an active connection"
            }
            Self::ManagementConnectFailed { .. } => {
                "Failed to connect to the OpenVPN Management Interface"
            }
            Self::ManagementTimeout => "The OpenVPN Management Interface timed out",
            Self::ManagementProtocolInvalid { .. } => {
                "OpenVPN returned an invalid management message"
            }
            Self::AuthenticationFailed => "OpenVPN authentication failed",
            Self::PermissionDenied { .. } => "RoutePilot does not have the required permission",
            Self::PrivilegedHelperUnavailable { .. } => {
                "Enable the RoutePilot system helper in Settings before connecting"
            }
            Self::PrivilegedHelperInstallFailed { .. } => {
                "The RoutePilot system helper could not be enabled"
            }
            Self::RouteConflict { .. } => "VPN routes overlap",
            Self::ProfileStoreCorrupted { .. } => "Stored VPN profile data is invalid",
            Self::SettingsCorrupted { .. } => "Stored application settings are invalid",
            Self::Io(_) => "A local I/O operation failed",
            Self::Unsupported { .. } => "This feature is not supported on this platform",
        }
    }

    #[must_use]
    pub fn to_payload(&self) -> ErrorPayload {
        ErrorPayload {
            code: self.code(),
            message: self.public_message(),
            details: self.public_details(),
        }
    }

    fn public_details(&self) -> Option<String> {
        match self {
            Self::ProfileNotFound { profile_id } => Some(format!("Profile ID: {profile_id}")),
            Self::ConnectionAlreadyActive { profile_id } => {
                Some(format!("Profile ID: {profile_id}"))
            }
            Self::PermissionDenied { operation } => Some(operation.clone()),
            Self::RouteConflict { details } => Some(details.clone()),
            Self::Unsupported { feature } => Some(feature.clone()),
            // Config, process, management, and I/O errors can contain profile
            // directives, commands, paths, or upstream output. Keep those details
            // in backend-only diagnostics.
            Self::ConfigInvalid { .. }
            | Self::OpenVpnInvalidExecutable { .. }
            | Self::OpenVpnStartFailed { .. }
            | Self::OpenVpnStopFailed { .. }
            | Self::ManagementConnectFailed { .. }
            | Self::ManagementTimeout
            | Self::ManagementProtocolInvalid { .. }
            | Self::OpenVpnNotFound
            | Self::AuthenticationFailed
            | Self::PrivilegedHelperUnavailable { .. }
            | Self::PrivilegedHelperInstallFailed { .. }
            | Self::ProfileStoreCorrupted { .. }
            | Self::SettingsCorrupted { .. }
            | Self::Io(_) => None,
        }
    }
}

/// Stable error shape returned by Tauri IPC commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: &'static str,
    pub message: &'static str,
    pub details: Option<String>,
}

impl From<&AppError> for ErrorPayload {
    fn from(error: &AppError) -> Self {
        error.to_payload()
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{AppError, ErrorPayload};

    #[test]
    fn maps_errors_to_stable_public_payloads() {
        let error = AppError::OpenVpnStartFailed {
            reason: "private backend diagnostics".to_owned(),
        };

        let payload = ErrorPayload::from(&error);

        assert_eq!(payload.code, "OPENVPN_START_FAILED");
        assert_eq!(payload.message, "Failed to start OpenVPN");
        assert_eq!(payload.details, None);
    }

    #[test]
    fn does_not_expose_nested_io_errors() {
        let error = AppError::from(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "/sensitive/profile/client.key",
        ));

        let payload = error.to_payload();
        let serialized = serde_json::to_string(&payload).expect("error payload should serialize");

        assert_eq!(payload.code, "IO_ERROR");
        assert!(!serialized.contains("client.key"));
        assert!(!serialized.contains("sensitive"));
    }

    #[test]
    fn does_not_expose_invalid_config_details() {
        let error = AppError::ConfigInvalid {
            reason: "inline private key content".to_owned(),
        };

        let payload = error.to_payload();

        assert_eq!(payload.code, "CONFIG_INVALID");
        assert_eq!(payload.details, None);
    }
}
