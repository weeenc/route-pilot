use std::{fmt, path::PathBuf};

use chrono::{DateTime, Utc};
use serde::{de, Deserialize, Deserializer, Serialize};

use crate::error::AppError;

pub const MAX_PROFILE_NAME_CHARACTERS: usize = 80;

/// Stable identifier used to associate persisted profiles with connection state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ProfileId(String);

impl ProfileId {
    pub fn new(value: impl Into<String>) -> Result<Self, AppError> {
        let value = value.into();
        let trimmed = value.trim();

        if trimmed.is_empty() {
            return Err(AppError::ConfigInvalid {
                reason: "profile ID cannot be empty".to_owned(),
            });
        }

        if trimmed.len() > 128
            || !trimmed.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
        {
            return Err(AppError::ConfigInvalid {
                reason: "profile ID contains unsupported characters".to_owned(),
            });
        }

        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProfileId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Persisted OpenVPN profile metadata.
///
/// Runtime process state belongs to [`super::VpnConnection`] and must never be
/// added to this model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnProfile {
    pub id: ProfileId,
    pub name: String,
    pub config_path: PathBuf,
    pub server_host: Option<String>,
    pub server_port: Option<u16>,
    pub protocol: Option<String>,
    pub auto_reconnect: bool,
    pub auto_connect: bool,
    pub ignore_redirect_gateway: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl VpnProfile {
    pub fn new(
        id: ProfileId,
        name: impl Into<String>,
        config_path: PathBuf,
    ) -> Result<Self, AppError> {
        Self::new_at(id, name, config_path, Utc::now())
    }

    pub fn new_at(
        id: ProfileId,
        name: impl Into<String>,
        config_path: PathBuf,
        now: DateTime<Utc>,
    ) -> Result<Self, AppError> {
        let name = normalize_profile_name(name.into())?;

        if config_path.as_os_str().is_empty() {
            return Err(AppError::ConfigInvalid {
                reason: "profile configuration path cannot be empty".to_owned(),
            });
        }

        Ok(Self {
            id,
            name,
            config_path,
            server_host: None,
            server_port: None,
            protocol: None,
            auto_reconnect: true,
            auto_connect: false,
            ignore_redirect_gateway: true,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update_editable_settings(
        &mut self,
        name: impl Into<String>,
        ignore_redirect_gateway: bool,
    ) -> Result<(), AppError> {
        self.update_editable_settings_at(name, ignore_redirect_gateway, Utc::now())
    }

    fn update_editable_settings_at(
        &mut self,
        name: impl Into<String>,
        ignore_redirect_gateway: bool,
        now: DateTime<Utc>,
    ) -> Result<(), AppError> {
        self.name = normalize_profile_name(name.into())?;
        self.ignore_redirect_gateway = ignore_redirect_gateway;
        self.updated_at = now;
        Ok(())
    }
}

fn normalize_profile_name(name: String) -> Result<String, AppError> {
    let trimmed_name = name.trim();

    if trimmed_name.is_empty() {
        return Err(AppError::ConfigInvalid {
            reason: "profile name cannot be empty".to_owned(),
        });
    }
    if trimmed_name.chars().count() > MAX_PROFILE_NAME_CHARACTERS {
        return Err(AppError::ConfigInvalid {
            reason: format!("profile name cannot exceed {MAX_PROFILE_NAME_CHARACTERS} characters"),
        });
    }
    if trimmed_name.chars().any(char::is_control) {
        return Err(AppError::ConfigInvalid {
            reason: "profile name cannot contain control characters".to_owned(),
        });
    }

    Ok(trimmed_name.to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::{TimeZone, Utc};

    use super::{ProfileId, VpnProfile, MAX_PROFILE_NAME_CHARACTERS};

    #[test]
    fn creates_profile_with_safe_routing_defaults() {
        let now = Utc
            .with_ymd_and_hms(2026, 8, 24, 10, 30, 0)
            .single()
            .expect("test timestamp should be valid");
        let profile = VpnProfile::new_at(
            ProfileId::new("vpn-a").expect("profile ID should be valid"),
            "  VPN A  ",
            PathBuf::from("profiles/vpn-a/config.ovpn"),
            now,
        )
        .expect("profile should be valid");

        assert_eq!(profile.name, "VPN A");
        assert!(profile.auto_reconnect);
        assert!(!profile.auto_connect);
        assert!(profile.ignore_redirect_gateway);
        assert_eq!(profile.created_at, now);
        assert_eq!(profile.updated_at, now);
    }

    #[test]
    fn rejects_empty_profile_identity_fields() {
        assert!(ProfileId::new("  ").is_err());
        assert!(ProfileId::new("../vpn-a").is_err());
        assert!(serde_json::from_str::<ProfileId>("\"  \"").is_err());

        let result = VpnProfile::new(
            ProfileId::new("vpn-a").expect("profile ID should be valid"),
            " ",
            PathBuf::from("config.ovpn"),
        );

        assert!(result.is_err());
    }

    #[test]
    fn updates_only_user_editable_profile_settings() {
        let created_at = Utc
            .with_ymd_and_hms(2026, 8, 24, 10, 30, 0)
            .single()
            .expect("test timestamp should be valid");
        let updated_at = Utc
            .with_ymd_and_hms(2026, 8, 25, 8, 15, 0)
            .single()
            .expect("test timestamp should be valid");
        let mut profile = VpnProfile::new_at(
            ProfileId::new("vpn-a").expect("profile ID should be valid"),
            "VPN A",
            PathBuf::from("profiles/vpn-a/config.ovpn"),
            created_at,
        )
        .expect("profile should be valid");

        profile
            .update_editable_settings_at("  Office VPN  ", false, updated_at)
            .expect("editable settings should update");

        assert_eq!(profile.name, "Office VPN");
        assert!(!profile.ignore_redirect_gateway);
        assert_eq!(profile.created_at, created_at);
        assert_eq!(profile.updated_at, updated_at);
        assert_eq!(
            profile.config_path,
            PathBuf::from("profiles/vpn-a/config.ovpn")
        );
    }

    #[test]
    fn rejects_invalid_profile_names_when_updating() {
        let mut profile = VpnProfile::new(
            ProfileId::new("vpn-a").expect("profile ID should be valid"),
            "VPN A",
            PathBuf::from("profiles/vpn-a/config.ovpn"),
        )
        .expect("profile should be valid");

        assert!(profile.update_editable_settings("   ", true).is_err());
        assert!(profile
            .update_editable_settings("a".repeat(MAX_PROFILE_NAME_CHARACTERS + 1), true)
            .is_err());
        assert!(profile
            .update_editable_settings("Office\nVPN", true)
            .is_err());
    }

    #[test]
    fn serializes_ipc_fields_as_camel_case_without_runtime_state() {
        let now = Utc
            .with_ymd_and_hms(2026, 8, 24, 10, 30, 0)
            .single()
            .expect("test timestamp should be valid");
        let profile = VpnProfile::new_at(
            ProfileId::new("vpn-a").expect("profile ID should be valid"),
            "VPN A",
            PathBuf::from("profiles/vpn-a/config.ovpn"),
            now,
        )
        .expect("profile should be valid");

        let value = serde_json::to_value(profile).expect("profile should serialize");

        assert_eq!(value["id"], "vpn-a");
        assert_eq!(value["configPath"], "profiles/vpn-a/config.ovpn");
        assert_eq!(value["autoReconnect"], true);
        assert_eq!(value["ignoreRedirectGateway"], true);
        assert!(value.get("state").is_none());
        assert!(value.get("config_path").is_none());
    }
}
