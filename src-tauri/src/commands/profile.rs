use std::{io, path::PathBuf, sync::MutexGuard};

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::{
    domain::{ProfileId, VpnProfile},
    error::{AppError, ErrorPayload},
    state::AppState,
    storage::ProfileStore,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProfileInput {
    name: String,
    ignore_redirect_gateway: bool,
}

fn profile_store(state: &AppState) -> Result<MutexGuard<'_, ProfileStore>, ErrorPayload> {
    state.profile_store.lock().map_err(|_| {
        AppError::from(io::Error::other("profile store lock is poisoned")).to_payload()
    })
}

fn blocking_task_error(error: impl std::fmt::Display) -> ErrorPayload {
    AppError::from(io::Error::other(format!("profile task failed: {error}"))).to_payload()
}

#[tauri::command]
pub async fn import_profile(
    source_path: PathBuf,
    app: AppHandle,
) -> Result<VpnProfile, ErrorPayload> {
    let task_app = app.clone();
    let profile = tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        let store = profile_store(state.inner())?;
        let profile = store
            .import_profile(&source_path)
            .map_err(|error| error.to_payload())?;
        state
            .cache_profile(profile.clone())
            .map_err(|error| error.to_payload())?;
        Ok::<_, ErrorPayload>(profile)
    })
    .await
    .map_err(blocking_task_error)??;
    #[cfg(desktop)]
    crate::tray::refresh_soon(app);
    Ok(profile)
}

#[tauri::command]
pub async fn list_profiles(app: AppHandle) -> Result<Vec<VpnProfile>, ErrorPayload> {
    app.state::<AppState>()
        .cached_profiles()
        .map_err(|error| error.to_payload())
}

#[tauri::command]
pub async fn get_profile(
    profile_id: ProfileId,
    app: AppHandle,
) -> Result<VpnProfile, ErrorPayload> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        profile_store(state.inner())?
            .get_profile(&profile_id)
            .map_err(|error| error.to_payload())
    })
    .await
    .map_err(blocking_task_error)?
}

#[tauri::command]
pub async fn update_profile(
    profile_id: ProfileId,
    input: UpdateProfileInput,
    app: AppHandle,
) -> Result<VpnProfile, ErrorPayload> {
    let task_app = app.clone();
    let profile = tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        let store = profile_store(state.inner())?;
        let profile = store
            .update_profile(&profile_id, input.name, input.ignore_redirect_gateway)
            .map_err(|error| error.to_payload())?;
        state
            .cache_profile(profile.clone())
            .map_err(|error| error.to_payload())?;
        Ok::<_, ErrorPayload>(profile)
    })
    .await
    .map_err(blocking_task_error)??;
    #[cfg(desktop)]
    crate::tray::refresh_soon(app);
    Ok(profile)
}

#[tauri::command]
pub async fn delete_profile(profile_id: ProfileId, app: AppHandle) -> Result<(), ErrorPayload> {
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        let store = profile_store(state.inner())?;
        store
            .delete_profile(&profile_id)
            .map_err(|error| error.to_payload())?;
        state
            .remove_cached_profile(&profile_id)
            .map_err(|error| error.to_payload())
    })
    .await
    .map_err(blocking_task_error)??;
    #[cfg(desktop)]
    crate::tray::refresh_soon(app);
    Ok(())
}
