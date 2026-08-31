use std::{io, path::PathBuf, sync::MutexGuard};

use tauri::{AppHandle, Manager};

use crate::{
    error::{AppError, ErrorPayload},
    state::AppState,
    storage::{AppSettings, SettingsStore},
    vpn::locator::LocatedOpenVpn,
};

fn settings_store(state: &AppState) -> Result<MutexGuard<'_, SettingsStore>, ErrorPayload> {
    state.settings_store.lock().map_err(|_| {
        AppError::from(io::Error::other("settings store lock is poisoned")).to_payload()
    })
}

fn blocking_task_error(error: impl std::fmt::Display) -> ErrorPayload {
    AppError::from(io::Error::other(format!("settings task failed: {error}"))).to_payload()
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<AppSettings, ErrorPayload> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        Ok(settings_store(state.inner())?.get())
    })
    .await
    .map_err(blocking_task_error)?
}

#[tauri::command]
pub async fn set_openvpn_executable(
    path: Option<PathBuf>,
    app: AppHandle,
) -> Result<AppSettings, ErrorPayload> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let validated_path = path
            .as_deref()
            .map(|path| state.openvpn_locator.validate_custom_path(path))
            .transpose()
            .map_err(|error| error.to_payload())?;

        settings_store(state.inner())?
            .set_openvpn_executable(validated_path)
            .map_err(|error| error.to_payload())
    })
    .await
    .map_err(blocking_task_error)?
}

#[tauri::command]
pub async fn set_check_for_updates_on_startup(
    enabled: bool,
    app: AppHandle,
) -> Result<AppSettings, ErrorPayload> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        settings_store(state.inner())?
            .set_check_for_updates_on_startup(enabled)
            .map_err(|error| error.to_payload())
    })
    .await
    .map_err(blocking_task_error)?
}

#[tauri::command]
pub async fn locate_openvpn(app: AppHandle) -> Result<LocatedOpenVpn, ErrorPayload> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let settings = settings_store(state.inner())?.get();
        state
            .openvpn_locator
            .locate(settings.openvpn_executable.as_deref())
            .map_err(|error| error.to_payload())
    })
    .await
    .map_err(blocking_task_error)?
}

#[tauri::command]
pub async fn get_privileged_helper_status() -> Result<serde_json::Value, ErrorPayload> {
    #[cfg(target_os = "macos")]
    {
        tauri::async_runtime::spawn_blocking(|| {
            serde_json::to_value(crate::vpn::privileged_helper::installation_status())
                .map_err(|error| AppError::from(io::Error::other(error)).to_payload())
        })
        .await
        .map_err(blocking_task_error)?
    }

    #[cfg(not(target_os = "macos"))]
    Ok(serde_json::json!({
        "state": "unsupported",
        "installedVersion": null,
        "expectedVersion": 0
    }))
}

#[tauri::command]
pub async fn enable_privileged_helper(app: AppHandle) -> Result<serde_json::Value, ErrorPayload> {
    #[cfg(target_os = "macos")]
    {
        let resource_directory = app
            .path()
            .resource_dir()
            .map_err(|error| AppError::from(io::Error::other(error)).to_payload())?;
        let status = tauri::async_runtime::spawn_blocking(move || {
            crate::vpn::privileged_helper::install(&resource_directory)
        })
        .await
        .map_err(|error| {
            AppError::PrivilegedHelperInstallFailed {
                reason: error.to_string(),
            }
            .to_payload()
        })?
        .map_err(|error| error.to_payload())?;
        serde_json::to_value(status)
            .map_err(|error| AppError::from(io::Error::other(error)).to_payload())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err(AppError::Unsupported {
            feature: "the RoutePilot macOS system helper".to_owned(),
        }
        .to_payload())
    }
}
