use std::{collections::HashMap, sync::Mutex, time::Duration};

use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    App, AppHandle, Manager,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::{
    commands::vpn::{connect_profile_inner, disconnect_profile_inner},
    domain::{ConnectionState, ProfileId, VpnConnection, VpnProfile},
    error::AppError,
    state::AppState,
};

const TRAY_ID: &str = "routepilot-status";
const SHOW_APP_ID: &str = "tray:show";
const QUIT_APP_ID: &str = "tray:quit";
const CONNECT_PREFIX: &str = "tray:connect:";
const DISCONNECT_PREFIX: &str = "tray:disconnect:";

struct TrayState {
    menu: Mutex<TrayMenuState>,
    refresh: tokio::sync::mpsc::UnboundedSender<()>,
}

#[derive(Default)]
struct TrayMenuState {
    signature: Option<String>,
    traffic_items: HashMap<ProfileId, MenuItem<tauri::Wry>>,
}

pub fn create(app: &App) -> tauri::Result<()> {
    let loading = MenuItem::with_id(
        app,
        "tray:loading",
        "正在读取连接信息…",
        false,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&loading])?;
    let icon = Image::from_bytes(include_bytes!("../icons-secure-route/32x32.png"))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("RoutePilot")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(handle_menu_event)
        .build(app)?;
    let (refresh_sender, mut refresh_requests) = tokio::sync::mpsc::unbounded_channel();
    app.manage(TrayState {
        menu: Mutex::new(TrayMenuState::default()),
        refresh: refresh_sender,
    });
    let app_handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        while refresh_requests.recv().await.is_some() {
            // State and traffic events from multiple profiles often arrive in the
            // same short burst. Drain them into one consistent tray snapshot.
            tokio::time::sleep(Duration::from_millis(100)).await;
            while refresh_requests.try_recv().is_ok() {}
            refresh(&app_handle).await;
        }
    });

    Ok(())
}

pub fn refresh_soon(app: AppHandle) {
    let _ = app.state::<TrayState>().refresh.send(());
}

async fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    let profiles = match state.cached_profiles() {
        Ok(profiles) => profiles,
        Err(_) => return,
    };
    let connections = state.vpn_manager.statuses().await;
    let signature = menu_signature(&profiles, &connections);
    let tray_state = app.state::<TrayState>();
    let existing_traffic_items = tray_state.menu.lock().ok().and_then(|menu| {
        (menu.signature.as_deref() == Some(signature.as_str())).then(|| menu.traffic_items.clone())
    });

    if let Some(traffic_items) = existing_traffic_items {
        update_traffic_items(&traffic_items, &connections);
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(connection_summary(&connections)));
        }
        return;
    }

    let Ok(built) = build_menu(app, &profiles, &connections) else {
        return;
    };
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if tray.set_menu(Some(built.menu)).is_err() {
        return;
    }
    let _ = tray.set_tooltip(Some(built.tooltip));
    if let Ok(mut menu_state) = tray_state.menu.lock() {
        menu_state.signature = Some(signature);
        menu_state.traffic_items = built.traffic_items;
    };
}

struct BuiltTrayMenu {
    menu: Menu<tauri::Wry>,
    tooltip: String,
    traffic_items: HashMap<ProfileId, MenuItem<tauri::Wry>>,
}

fn build_menu(
    app: &AppHandle,
    profiles: &[VpnProfile],
    connections: &[VpnConnection],
) -> tauri::Result<BuiltTrayMenu> {
    let menu = Menu::new(app)?;
    let mut traffic_items = HashMap::new();
    let connections_by_profile = connections
        .iter()
        .map(|connection| (&connection.profile_id, connection))
        .collect::<HashMap<_, _>>();
    let summary = connection_summary(connections);
    let summary_item = disabled_item(app, "tray:summary", &summary)?;
    menu.append(&summary_item)?;

    let mut displayed_connection = false;
    for (index, profile) in profiles.iter().enumerate() {
        let Some(connection) = connections_by_profile.get(&profile.id) else {
            continue;
        };
        if connection.state == ConnectionState::Disconnected {
            continue;
        }

        menu.append(&PredefinedMenuItem::separator(app)?)?;
        displayed_connection = true;
        append_connection_info(&menu, app, profile, connection, index, &mut traffic_items)?;
    }

    let available_profiles = profiles
        .iter()
        .filter(|profile| {
            connections_by_profile
                .get(&profile.id)
                .map_or(true, |connection| {
                    matches!(
                        connection.state,
                        ConnectionState::Disconnected | ConnectionState::Error
                    )
                })
        })
        .collect::<Vec<_>>();

    if !available_profiles.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        for profile in available_profiles {
            let label = if connections_by_profile
                .get(&profile.id)
                .is_some_and(|connection| connection.state == ConnectionState::Error)
            {
                format!("重新连接 {}", escape_menu_text(&profile.name))
            } else {
                format!("连接 {}", escape_menu_text(&profile.name))
            };
            let item = MenuItem::with_id(
                app,
                format!("{CONNECT_PREFIX}{}", profile.id),
                label,
                true,
                None::<&str>,
            )?;
            menu.append(&item)?;
        }
    } else if profiles.is_empty() && !displayed_connection {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&disabled_item(
            app,
            "tray:no-profiles",
            "尚未导入 VPN 配置",
        )?)?;
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let show = MenuItem::with_id(app, SHOW_APP_ID, "打开 RoutePilot", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_APP_ID, "退出 RoutePilot", true, None::<&str>)?;
    menu.append(&show)?;
    menu.append(&quit)?;

    Ok(BuiltTrayMenu {
        menu,
        tooltip: summary,
        traffic_items,
    })
}

fn append_connection_info(
    menu: &Menu<tauri::Wry>,
    app: &AppHandle,
    profile: &VpnProfile,
    connection: &VpnConnection,
    index: usize,
    traffic_items: &mut HashMap<ProfileId, MenuItem<tauri::Wry>>,
) -> tauri::Result<()> {
    let profile_name = escape_menu_text(&profile.name);
    let status = connection_state_label(connection.state);
    menu.append(&disabled_item(
        app,
        &format!("tray:info:{index}:status"),
        &format!("● {profile_name} · {status}"),
    )?)?;

    if let Some(remote_address) = connection.remote_address {
        menu.append(&disabled_item(
            app,
            &format!("tray:info:{index}:remote"),
            &format!("远端 IP：{remote_address}"),
        )?)?;
    }
    if let Some(tunnel_address) = connection.tunnel_address {
        menu.append(&disabled_item(
            app,
            &format!("tray:info:{index}:tunnel"),
            &format!("VPN IP：{tunnel_address}"),
        )?)?;
    }
    if matches!(
        connection.state,
        ConnectionState::Connected | ConnectionState::Reconnecting
    ) {
        let traffic_item = disabled_item(
            app,
            &format!("tray:info:{index}:traffic"),
            &traffic_label(connection),
        )?;
        menu.append(&traffic_item)?;
        traffic_items.insert(profile.id.clone(), traffic_item);
    }
    if let Some(error) = connection.error_message.as_deref() {
        menu.append(&disabled_item(
            app,
            &format!("tray:info:{index}:error"),
            &format!("错误：{}", truncate(error, 72)),
        )?)?;
    }

    if connection.state != ConnectionState::Error {
        let disconnect = MenuItem::with_id(
            app,
            format!("{DISCONNECT_PREFIX}{}", profile.id),
            format!("断开 {profile_name}"),
            connection.state != ConnectionState::Disconnecting,
            None::<&str>,
        )?;
        menu.append(&disconnect)?;
    }

    Ok(())
}

fn disabled_item(app: &AppHandle, id: &str, label: &str) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(app, id, label, false, None::<&str>)
}

fn connection_summary(connections: &[VpnConnection]) -> String {
    let connected_count = connections
        .iter()
        .filter(|connection| {
            matches!(
                connection.state,
                ConnectionState::Connected | ConnectionState::Reconnecting
            )
        })
        .count();
    if connected_count > 0 {
        format!("RoutePilot · 已连接 {connected_count} 个")
    } else if connections
        .iter()
        .any(|connection| connection.state == ConnectionState::Connecting)
    {
        "RoutePilot · 正在连接…".to_owned()
    } else {
        "RoutePilot · 当前未连接".to_owned()
    }
}

fn menu_signature(profiles: &[VpnProfile], connections: &[VpnConnection]) -> String {
    let profile_snapshot = profiles
        .iter()
        .map(|profile| (&profile.id, &profile.name))
        .collect::<Vec<_>>();
    let connection_snapshot = connections
        .iter()
        .map(|connection| {
            (
                &connection.profile_id,
                connection.state,
                connection.remote_address,
                connection.tunnel_address,
                &connection.error_message,
            )
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&(profile_snapshot, connection_snapshot)).unwrap_or_default()
}

fn update_traffic_items(
    traffic_items: &HashMap<ProfileId, MenuItem<tauri::Wry>>,
    connections: &[VpnConnection],
) {
    for connection in connections {
        if let Some(item) = traffic_items.get(&connection.profile_id) {
            let _ = item.set_text(traffic_label(connection));
        }
    }
}

fn traffic_label(connection: &VpnConnection) -> String {
    format!(
        "下载 {}  ·  上传 {}",
        format_bytes(connection.bytes_received),
        format_bytes(connection.bytes_sent)
    )
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref();
    if id == SHOW_APP_ID {
        show_main_window(app);
        return;
    }
    if id == QUIT_APP_ID {
        app.exit(0);
        return;
    }

    let action = if let Some(profile_id) = id.strip_prefix(CONNECT_PREFIX) {
        Some((profile_id, true))
    } else {
        id.strip_prefix(DISCONNECT_PREFIX)
            .map(|profile_id| (profile_id, false))
    };
    let Some((profile_id, should_connect)) = action else {
        return;
    };
    let Ok(profile_id) = ProfileId::new(profile_id) else {
        return;
    };

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let result = if should_connect {
            connect_profile_inner(profile_id, state.inner()).await
        } else {
            disconnect_profile_inner(profile_id, state.inner()).await
        };

        if let Err(error) = result {
            show_action_error(&app, &error);
        }
        refresh_soon(app);
    });
}

pub fn show_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_dock_visibility(true);

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

fn show_action_error(app: &AppHandle, error: &AppError) {
    app.dialog()
        .message(action_error_message(error))
        .title("RoutePilot")
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}

fn action_error_message(error: &AppError) -> &'static str {
    match error {
        AppError::OpenVpnNotFound | AppError::OpenVpnInvalidExecutable { .. } => {
            "未找到可用的 OpenVPN，请打开 RoutePilot 设置进行配置。"
        }
        AppError::PrivilegedHelperUnavailable { .. } => {
            "请先在 RoutePilot 设置中启用 VPN 系统辅助程序。"
        }
        AppError::AuthenticationFailed => "OpenVPN 身份验证失败，请检查 VPN 配置。",
        AppError::ConnectionAlreadyActive { .. } => "此 VPN 配置已有活动连接。",
        AppError::ProfileNotFound { .. } => "找不到该 VPN 配置，请重新打开菜单后再试。",
        _ => "VPN 操作失败，请打开 RoutePilot 查看连接详情。",
    }
}

fn connection_state_label(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Disconnected => "未连接",
        ConnectionState::Connecting => "正在连接",
        ConnectionState::Connected => "已连接",
        ConnectionState::Reconnecting => "正在重连",
        ConnectionState::Disconnecting => "正在断开",
        ConnectionState::Error => "连接失败",
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

fn escape_menu_text(text: &str) -> String {
    text.replace('&', "&&")
}

fn truncate(text: &str, maximum_characters: usize) -> String {
    let mut characters = text.chars();
    let truncated = characters
        .by_ref()
        .take(maximum_characters)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{ConnectionState, ProfileId, VpnConnection};

    use super::{format_bytes, menu_signature, truncate};

    #[test]
    fn formats_traffic_for_compact_menu_display() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
    }

    #[test]
    fn truncates_menu_errors_at_character_boundaries() {
        assert_eq!(truncate("连接失败，请重试", 4), "连接失败…");
        assert_eq!(truncate("short", 10), "short");
    }

    #[test]
    fn ignores_traffic_only_changes_when_deciding_to_replace_the_menu() {
        let profile_id = ProfileId::new("vpn-a").expect("profile ID should be valid");
        let mut connection = VpnConnection::disconnected(profile_id);
        connection.state = ConnectionState::Connected;
        let original = menu_signature(&[], &[connection.clone()]);

        connection.bytes_received = 4096;
        connection.bytes_sent = 2048;
        assert_eq!(menu_signature(&[], &[connection.clone()]), original);

        connection.state = ConnectionState::Reconnecting;
        assert_ne!(menu_signature(&[], &[connection]), original);
    }
}
