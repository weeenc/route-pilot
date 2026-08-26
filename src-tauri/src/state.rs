use std::{
    io,
    sync::{Mutex, RwLock},
};

use crate::{
    domain::{ProfileId, VpnProfile},
    error::AppError,
    storage::{ProfileStore, SettingsStore},
    vpn::{locator::OpenVpnLocator, manager::VpnManager},
};

pub struct AppState {
    pub profile_store: Mutex<ProfileStore>,
    profiles: RwLock<Vec<VpnProfile>>,
    pub settings_store: Mutex<SettingsStore>,
    pub openvpn_locator: OpenVpnLocator,
    pub vpn_manager: VpnManager,
}

impl AppState {
    pub fn new(
        profile_store: ProfileStore,
        profiles: Vec<VpnProfile>,
        settings_store: SettingsStore,
        openvpn_locator: OpenVpnLocator,
        vpn_manager: VpnManager,
    ) -> Self {
        Self {
            profile_store: Mutex::new(profile_store),
            profiles: RwLock::new(profiles),
            settings_store: Mutex::new(settings_store),
            openvpn_locator,
            vpn_manager,
        }
    }

    pub fn cached_profiles(&self) -> Result<Vec<VpnProfile>, AppError> {
        self.profiles
            .read()
            .map(|profiles| profiles.clone())
            .map_err(|_| AppError::from(io::Error::other("profile cache lock is poisoned")))
    }

    pub fn cache_profile(&self, profile: VpnProfile) -> Result<(), AppError> {
        let mut profiles = self
            .profiles
            .write()
            .map_err(|_| AppError::from(io::Error::other("profile cache lock is poisoned")))?;
        if let Some(existing) = profiles.iter_mut().find(|item| item.id == profile.id) {
            *existing = profile;
        } else {
            profiles.push(profile);
        }
        profiles.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        });
        Ok(())
    }

    pub fn remove_cached_profile(&self, profile_id: &ProfileId) -> Result<(), AppError> {
        let mut profiles = self
            .profiles
            .write()
            .map_err(|_| AppError::from(io::Error::other("profile cache lock is poisoned")))?;
        profiles.retain(|profile| &profile.id != profile_id);
        Ok(())
    }
}
