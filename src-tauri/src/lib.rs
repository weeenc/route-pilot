pub mod commands;
pub mod domain;
pub mod error;
pub mod platform;
pub mod storage;
#[cfg(desktop)]
pub mod tray;
pub mod vpn;

pub use error::{AppError, ErrorPayload};

mod state;

use state::AppState;
use storage::{ProfileStore, SettingsStore};
use tauri::{Emitter, Manager};
use vpn::{
    locator::OpenVpnLocator,
    manager::{VpnManager, CONNECTION_UPDATED_EVENT},
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let resource_dir = app.path().resource_dir()?;
            let profile_store = ProfileStore::new(app_data_dir.clone())?;
            let profiles = profile_store.list_profiles()?;
            let settings_store = SettingsStore::new(app_data_dir)?;
            let openvpn_locator = OpenVpnLocator::new(resource_dir);
            let vpn_manager = VpnManager::new();
            let mut connection_events = vpn_manager.subscribe();
            app.manage(AppState::new(
                profile_store,
                profiles,
                settings_store,
                openvpn_locator,
                vpn_manager,
            ));

            #[cfg(desktop)]
            {
                tray::create(app)?;
                tray::refresh_soon(app.handle().clone());
            }

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match connection_events.recv().await {
                        Ok(connection) => {
                            let _ = app_handle.emit(CONNECTION_UPDATED_EVENT, connection);
                            #[cfg(desktop)]
                            tray::refresh_soon(app_handle.clone());
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::profile::import_profile,
            commands::profile::list_profiles,
            commands::profile::get_profile,
            commands::profile::update_profile,
            commands::profile::delete_profile,
            commands::settings::get_settings,
            commands::settings::set_openvpn_executable,
            commands::settings::locate_openvpn,
            commands::settings::get_privileged_helper_status,
            commands::settings::enable_privileged_helper,
            commands::vpn::connect_profile,
            commands::vpn::disconnect_profile,
            commands::vpn::get_connection,
            commands::vpn::list_connections,
            commands::vpn::list_route_conflicts,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build RoutePilot");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "main" => {
            api.prevent_close();
            if let Some(window) = app_handle.get_webview_window(&label) {
                let _ = window.hide();
            }
            #[cfg(target_os = "macos")]
            let _ = app_handle.set_dock_visibility(false);
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => tray::show_main_window(app_handle),
        tauri::RunEvent::Exit => {
            let state = app_handle.state::<AppState>();
            let _ = tauri::async_runtime::block_on(state.vpn_manager.shutdown_all());
        }
        _ => {}
    });
}
