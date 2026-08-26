use tauri::State;

use crate::{
    domain::{ProfileId, RouteConflict, VpnConnection, VpnProfile},
    error::{AppError, ErrorPayload},
    state::AppState,
    storage::{AppSettings, SettingsStore},
};

fn settings_store(state: &AppState) -> Result<std::sync::MutexGuard<'_, SettingsStore>, AppError> {
    state
        .settings_store
        .lock()
        .map_err(|_| AppError::from(std::io::Error::other("settings store lock is poisoned")))
}

pub(crate) async fn connect_profile_inner(
    profile_id: ProfileId,
    state: &AppState,
) -> Result<VpnConnection, AppError> {
    let profile: VpnProfile = state
        .cached_profiles()?
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| AppError::ProfileNotFound {
            profile_id: profile_id.to_string(),
        })?;
    let settings: AppSettings = { settings_store(state)?.get() };
    let executable = state
        .openvpn_locator
        .locate(settings.openvpn_executable.as_deref())?;

    state.vpn_manager.start(&profile, &executable.path).await
}

pub(crate) async fn disconnect_profile_inner(
    profile_id: ProfileId,
    state: &AppState,
) -> Result<VpnConnection, AppError> {
    state.vpn_manager.stop(&profile_id).await
}

#[tauri::command]
pub async fn connect_profile(
    profile_id: ProfileId,
    state: State<'_, AppState>,
) -> Result<VpnConnection, ErrorPayload> {
    connect_profile_inner(profile_id, state.inner())
        .await
        .map_err(|error| error.to_payload())
}

#[tauri::command]
pub async fn disconnect_profile(
    profile_id: ProfileId,
    state: State<'_, AppState>,
) -> Result<VpnConnection, ErrorPayload> {
    disconnect_profile_inner(profile_id, state.inner())
        .await
        .map_err(|error| error.to_payload())
}

#[tauri::command]
pub async fn get_connection(
    profile_id: ProfileId,
    state: State<'_, AppState>,
) -> Result<VpnConnection, ErrorPayload> {
    Ok(state.vpn_manager.status(&profile_id).await)
}

#[tauri::command]
pub async fn list_connections(
    state: State<'_, AppState>,
) -> Result<Vec<VpnConnection>, ErrorPayload> {
    Ok(state.vpn_manager.statuses().await)
}

#[tauri::command]
pub async fn list_route_conflicts(
    state: State<'_, AppState>,
) -> Result<Vec<RouteConflict>, ErrorPayload> {
    Ok(state.vpn_manager.route_conflicts().await)
}
