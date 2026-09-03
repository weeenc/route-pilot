use std::{
    collections::HashMap,
    net::Ipv4Addr,
    path::Path,
    sync::{Arc, Weak},
    time::Duration,
};

use chrono::{DateTime, Utc};
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc, oneshot, watch, Mutex, RwLock},
    task::JoinHandle,
    time::{interval, sleep, timeout},
};

use crate::{
    domain::{
        detect_route_conflicts, ConnectionState, ProfileId, Route, RouteConflict, VpnConnection,
        VpnProfile,
    },
    error::AppError,
};

use super::{
    management::{ManagementClient, ManagementEvent, ManagementStateUpdate},
    process::{OpenVpnLaunchConfig, OpenVpnManagementOptions, OpenVpnProcess},
    routing::RuntimeConfig,
};

const MANAGEMENT_CONNECT_ATTEMPTS: usize = 100;
const MANAGEMENT_CONNECT_RETRY_DELAY: Duration = Duration::from_millis(50);
const MANAGEMENT_STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_STATUS_INTERVAL: Duration = Duration::from_millis(250);
const RUNTIME_COMMAND_CAPACITY: usize = 8;
const CONNECTION_EVENT_CAPACITY: usize = 256;

pub const CONNECTION_UPDATED_EVENT: &str = "vpn://connection-updated";

/// Owns all active per-profile VPN runtimes.
///
/// Runtime processes are isolated behind per-profile actor tasks. The map only
/// stores control and state handles, so stopping one profile never locks or
/// mutates another profile's process or Management Interface.
pub struct VpnManager {
    runtimes: RwLock<HashMap<ProfileId, VpnRuntime>>,
    operations: Mutex<HashMap<ProfileId, Weak<Mutex<()>>>>,
    events: broadcast::Sender<VpnConnection>,
}

impl Default for VpnManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VpnManager {
    #[must_use]
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(CONNECTION_EVENT_CAPACITY);
        Self {
            runtimes: RwLock::new(HashMap::new()),
            operations: Mutex::new(HashMap::new()),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<VpnConnection> {
        self.events.subscribe()
    }

    pub async fn start(
        &self,
        profile: &VpnProfile,
        executable: &Path,
    ) -> Result<VpnConnection, AppError> {
        let _operation = self.profile_operation(&profile.id).await;
        self.ensure_not_active(&profile.id).await?;

        let runtime_config = RuntimeConfig::create(
            &profile.config_path,
            profile.ignore_redirect_gateway,
            &profile.split_tunnel_domains,
        )
        .await?;
        let initial_routes = runtime_config.routes().to_vec();

        let management_listener = allocate_management_listener().await?;
        let management_port = management_listener
            .local_addr()
            .map_err(management_error)?
            .port();
        let management_options = OpenVpnManagementOptions::new(management_port)?;
        let launch_config =
            OpenVpnLaunchConfig::new(profile.id.clone(), executable, runtime_config.path())?
                .with_management(management_options);
        let mut process = OpenVpnProcess::start(launch_config).await?;
        let management = match accept_management(&mut process, management_listener).await {
            Ok(management) => management,
            Err(error) => {
                let _ = process.stop().await;
                return Err(error);
            }
        };

        self.insert_runtime(
            profile.id.clone(),
            process,
            management,
            management_port,
            initial_routes,
            Some(runtime_config),
        )
        .await
    }

    pub async fn stop(&self, profile_id: &ProfileId) -> Result<VpnConnection, AppError> {
        let _operation = self.profile_operation(profile_id).await;
        let runtime_control = {
            let runtimes = self.runtimes.read().await;
            runtimes
                .get(profile_id)
                .map(|runtime| runtime.control.clone())
        };
        let Some(runtime_control) = runtime_control else {
            return Ok(VpnConnection::disconnected(profile_id.clone()));
        };

        let (response_sender, response_receiver) = oneshot::channel();
        if runtime_control
            .send(RuntimeCommand::Stop {
                response: response_sender,
            })
            .await
            .is_err()
        {
            self.remove_and_join(profile_id).await?;
            return Ok(VpnConnection::disconnected(profile_id.clone()));
        }

        let stop_result = response_receiver
            .await
            .map_err(|_| AppError::OpenVpnStopFailed {
                reason: "VPN runtime stopped before confirming process cleanup".to_owned(),
            });
        let join_result = self.remove_and_join(profile_id).await;

        match (stop_result, join_result) {
            (Ok(result), Ok(())) => result,
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }

    #[must_use]
    pub async fn status(&self, profile_id: &ProfileId) -> VpnConnection {
        let runtimes = self.runtimes.read().await;
        match runtimes.get(profile_id) {
            Some(runtime) => runtime.status.borrow().clone(),
            None => VpnConnection::disconnected(profile_id.clone()),
        }
    }

    #[must_use]
    pub async fn statuses(&self) -> Vec<VpnConnection> {
        let runtimes = self.runtimes.read().await;
        let mut connections = runtimes
            .values()
            .map(|runtime| runtime.status.borrow().clone())
            .collect::<Vec<_>>();
        connections.sort_by(|left, right| left.profile_id.as_str().cmp(right.profile_id.as_str()));
        connections
    }

    #[must_use]
    pub async fn route_conflicts(&self) -> Vec<RouteConflict> {
        detect_route_conflicts(&self.statuses().await)
    }

    /// Stops every owned process before the application runtime exits.
    pub async fn shutdown_all(&self) -> Result<(), AppError> {
        let profile_ids = self
            .runtimes
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let results =
            futures::future::join_all(profile_ids.iter().map(|profile_id| self.stop(profile_id)))
                .await;
        let first_error = results.into_iter().find_map(Result::err);

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn profile_operation(&self, profile_id: &ProfileId) -> tokio::sync::OwnedMutexGuard<()> {
        let operation = {
            let mut operations = self.operations.lock().await;
            operations.retain(|_, operation| operation.strong_count() > 0);
            match operations.get(profile_id).and_then(Weak::upgrade) {
                Some(operation) => operation,
                None => {
                    let operation = Arc::new(Mutex::new(()));
                    operations.insert(profile_id.clone(), Arc::downgrade(&operation));
                    operation
                }
            }
        };
        operation.lock_owned().await
    }

    async fn ensure_not_active(&self, profile_id: &ProfileId) -> Result<(), AppError> {
        let finished = self
            .runtimes
            .read()
            .await
            .get(profile_id)
            .is_some_and(|runtime| runtime.task.is_finished());
        if finished {
            self.remove_and_join(profile_id).await?;
        }
        if self.runtimes.read().await.contains_key(profile_id) {
            return Err(AppError::ConnectionAlreadyActive {
                profile_id: profile_id.to_string(),
            });
        }
        Ok(())
    }

    async fn insert_runtime(
        &self,
        profile_id: ProfileId,
        process: OpenVpnProcess,
        management: ManagementClient,
        management_port: u16,
        initial_routes: Vec<Route>,
        runtime_config: Option<RuntimeConfig>,
    ) -> Result<VpnConnection, AppError> {
        self.ensure_not_active(&profile_id).await?;

        let mut connection = VpnConnection::disconnected(profile_id.clone());
        connection.state = ConnectionState::Connecting;
        connection.process_id = process.process_id();
        connection.management_port = Some(management_port);
        connection.routes = initial_routes;

        let (status_sender, status) = watch::channel(connection.clone());
        let publisher = RuntimePublisher {
            status: status_sender,
            events: self.events.clone(),
        };
        let (control, control_receiver) = mpsc::channel(RUNTIME_COMMAND_CAPACITY);
        let _ = self.events.send(connection.clone());
        let ignores_server_routes = runtime_config
            .as_ref()
            .is_some_and(RuntimeConfig::ignores_server_routes);
        let task = tokio::spawn(run_runtime(
            process,
            management,
            connection.clone(),
            publisher,
            control_receiver,
            ignores_server_routes,
            runtime_config,
        ));
        let runtime = VpnRuntime {
            status,
            control,
            task,
        };
        self.runtimes.write().await.insert(profile_id, runtime);

        Ok(connection)
    }

    async fn remove_and_join(&self, profile_id: &ProfileId) -> Result<(), AppError> {
        let runtime = self.runtimes.write().await.remove(profile_id);
        let Some(runtime) = runtime else {
            return Ok(());
        };

        runtime
            .task
            .await
            .map_err(|error| AppError::OpenVpnStopFailed {
                reason: format!("VPN runtime task failed: {error}"),
            })
    }
}

struct VpnRuntime {
    status: watch::Receiver<VpnConnection>,
    control: mpsc::Sender<RuntimeCommand>,
    task: JoinHandle<()>,
}

struct RuntimePublisher {
    status: watch::Sender<VpnConnection>,
    events: broadcast::Sender<VpnConnection>,
}

impl RuntimePublisher {
    fn publish(&self, connection: &VpnConnection) {
        self.status.send_replace(connection.clone());
        let _ = self.events.send(connection.clone());
    }
}

enum RuntimeCommand {
    Stop {
        response: oneshot::Sender<Result<VpnConnection, AppError>>,
    },
}

async fn allocate_management_listener() -> Result<TcpListener, AppError> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(management_error)
}

async fn accept_management(
    process: &mut OpenVpnProcess,
    listener: TcpListener,
) -> Result<ManagementClient, AppError> {
    timeout(
        MANAGEMENT_STARTUP_TIMEOUT,
        accept_management_until_available(process, listener),
    )
    .await
    .map_err(|_| AppError::ManagementTimeout)?
}

async fn accept_management_until_available(
    process: &mut OpenVpnProcess,
    listener: TcpListener,
) -> Result<ManagementClient, AppError> {
    let port = listener.local_addr().map_err(management_error)?.port();
    let accept = listener.accept();
    tokio::pin!(accept);

    for attempt in 0..MANAGEMENT_CONNECT_ATTEMPTS {
        if !process.is_running()? {
            return Err(AppError::OpenVpnStartFailed {
                reason: "OpenVPN exited before its Management Interface became available"
                    .to_owned(),
            });
        }

        tokio::select! {
            accepted = &mut accept => {
                let (stream, peer) = accepted.map_err(management_error)?;
                if !peer.ip().is_loopback() {
                    return Err(AppError::ManagementConnectFailed {
                        reason: "OpenVPN Management Interface connected from outside loopback".to_owned(),
                    });
                }
                return ManagementClient::from_stream(stream, port).await;
            }
            () = sleep(MANAGEMENT_CONNECT_RETRY_DELAY), if attempt + 1 < MANAGEMENT_CONNECT_ATTEMPTS => {}
        }
    }

    Err(AppError::ManagementConnectFailed {
        reason: "Management Interface did not connect to RoutePilot".to_owned(),
    })
}

async fn run_runtime(
    mut process: OpenVpnProcess,
    mut management: ManagementClient,
    mut connection: VpnConnection,
    publisher: RuntimePublisher,
    mut commands: mpsc::Receiver<RuntimeCommand>,
    ignores_server_routes: bool,
    _runtime_config: Option<RuntimeConfig>,
) {
    let mut process_status = interval(PROCESS_STATUS_INTERVAL);

    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(RuntimeCommand::Stop { response }) = command else {
                    stop_after_control_channel_closed(&mut process, management, &mut connection, &publisher).await;
                    break;
                };
                connection.state = ConnectionState::Disconnecting;
                publisher.publish(&connection);

                let _ = management.terminate().await;
                let result = match process.stop().await {
                    Ok(_) => {
                        mark_disconnected(&mut connection);
                        publisher.publish(&connection);
                        Ok(connection.clone())
                    }
                    Err(error) => {
                        connection.state = ConnectionState::Error;
                        publisher.publish(&connection);
                        Err(error)
                    }
                };
                let _ = response.send(result);
                break;
            }
            event = management.next_event() => {
                match event {
                    Ok(Some(event)) => {
                        apply_management_event(&mut connection, event, ignores_server_routes);
                        publisher.publish(&connection);
                    }
                    Ok(None) | Err(_) => {
                        connection.state = ConnectionState::Error;
                        connection.error_message.get_or_insert_with(|| {
                            "The OpenVPN connection ended unexpectedly.".to_owned()
                        });
                        connection.connected_at = None;
                        connection.tunnel_address = None;
                        connection.remote_address = None;
                        connection.routes.clear();
                        let _ = management.terminate().await;
                        let _ = process.stop().await;
                        connection.process_id = None;
                        connection.management_port = None;
                        publisher.publish(&connection);
                        break;
                    }
                }
            }
            _ = process_status.tick() => {
                match process.is_running() {
                    Ok(true) => {}
                    Ok(false) => {
                        if process.exit_status().is_some_and(|exit| exit.success) {
                            mark_disconnected(&mut connection);
                        } else {
                            connection.state = ConnectionState::Error;
                            connection.error_message.get_or_insert_with(|| {
                                "OpenVPN exited before the connection was established.".to_owned()
                            });
                            connection.connected_at = None;
                            connection.tunnel_address = None;
                            connection.remote_address = None;
                            connection.process_id = None;
                            connection.management_port = None;
                            connection.routes.clear();
                        }
                        publisher.publish(&connection);
                        let _ = management.disconnect().await;
                        break;
                    }
                    Err(_) => {
                        connection.state = ConnectionState::Error;
                        connection.error_message = Some(
                            "RoutePilot could not read the OpenVPN process status.".to_owned(),
                        );
                        connection.connected_at = None;
                        connection.tunnel_address = None;
                        connection.remote_address = None;
                        connection.routes.clear();
                        publisher.publish(&connection);
                        let _ = management.terminate().await;
                        let _ = process.stop().await;
                        connection.process_id = None;
                        connection.management_port = None;
                        publisher.publish(&connection);
                        break;
                    }
                }
            }
        }
    }
}

async fn stop_after_control_channel_closed(
    process: &mut OpenVpnProcess,
    management: ManagementClient,
    connection: &mut VpnConnection,
    publisher: &RuntimePublisher,
) {
    connection.state = ConnectionState::Disconnecting;
    publisher.publish(connection);
    let _ = management.terminate().await;
    if process.stop().await.is_ok() {
        mark_disconnected(connection);
    } else {
        connection.state = ConnectionState::Error;
    }
    publisher.publish(connection);
}

fn apply_management_event(
    connection: &mut VpnConnection,
    event: ManagementEvent,
    ignores_server_routes: bool,
) {
    match event {
        ManagementEvent::State(update) => apply_state_update(connection, update),
        ManagementEvent::ByteCount(byte_count) => {
            connection.bytes_received = byte_count.bytes_received;
            connection.bytes_sent = byte_count.bytes_sent;
        }
        ManagementEvent::PushReply(reply) => {
            if ignores_server_routes {
                return;
            }
            for route in reply.routes {
                if !connection.routes.contains(&route) {
                    connection.routes.push(route);
                }
            }
        }
    }
}

fn apply_state_update(connection: &mut VpnConnection, update: ManagementStateUpdate) {
    if update.description.as_deref() == Some("auth-failure") {
        connection.state = ConnectionState::Error;
        connection.error_message = Some(
            "Authentication failed. This VPN client certificate was rejected by the server."
                .to_owned(),
        );
        connection.connected_at = None;
        connection.tunnel_address = None;
        connection.remote_address = None;
        return;
    }
    if update.state == ConnectionState::Reconnecting {
        connection
            .routes
            .retain(|route| route.source != crate::domain::RouteSource::ServerPush);
    }
    connection.state = update.state;
    if update.state == ConnectionState::Connected {
        connection.error_message = None;
        connection.tunnel_address = update.tunnel_address;
        connection.remote_address = update.remote_address;
        if connection.connected_at.is_none() {
            connection.connected_at =
                DateTime::<Utc>::from_timestamp(update.timestamp, 0).or_else(|| Some(Utc::now()));
        }
    } else {
        connection.connected_at = None;
        connection.tunnel_address = None;
        connection.remote_address = None;
    }
}

fn mark_disconnected(connection: &mut VpnConnection) {
    connection.state = ConnectionState::Disconnected;
    connection.process_id = None;
    connection.management_port = None;
    connection.connected_at = None;
    connection.error_message = None;
    connection.tunnel_address = None;
    connection.remote_address = None;
    connection.routes.clear();
}

fn management_error(error: std::io::Error) -> AppError {
    AppError::ManagementConnectFailed {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use tokio::net::TcpListener;

    #[cfg(unix)]
    use std::{fs, net::IpAddr, time::Duration};

    #[cfg(unix)]
    use tempfile::TempDir;
    #[cfg(unix)]
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        process::Command,
        task::JoinHandle,
        time::timeout,
    };

    #[cfg(unix)]
    use crate::{
        domain::{ConnectionState, ProfileId},
        error::AppError,
        vpn::{
            management::{ManagementClient, ManagementStateUpdate},
            process::OpenVpnProcess,
        },
    };

    use super::allocate_management_listener;
    #[cfg(unix)]
    use super::apply_state_update;
    #[cfg(unix)]
    use super::VpnManager;

    #[tokio::test]
    async fn reserves_an_available_dynamic_loopback_port_until_process_start() {
        let listener = allocate_management_listener()
            .await
            .expect("management listener should be allocated");
        let address = listener
            .local_addr()
            .expect("listener address should be available");

        assert_ne!(address.port(), 0);
        assert_eq!(address.ip(), Ipv4Addr::LOCALHOST);
        assert!(TcpListener::bind(address).await.is_err());

        drop(listener);
        let replacement = TcpListener::bind(address)
            .await
            .expect("released port should become available on loopback");
        drop(replacement);
    }

    #[cfg(unix)]
    #[test]
    fn reports_server_authentication_rejection() {
        let profile_id = ProfileId::new("vpn-auth-failure").expect("profile ID should be valid");
        let mut connection = crate::domain::VpnConnection::disconnected(profile_id);

        apply_state_update(
            &mut connection,
            ManagementStateUpdate {
                timestamp: 1_700_000_000,
                state: ConnectionState::Disconnecting,
                description: Some("auth-failure".to_owned()),
                tunnel_address: None,
                remote_address: None,
            },
        );

        assert_eq!(connection.state, ConnectionState::Error);
        assert_eq!(
            connection.error_message.as_deref(),
            Some("Authentication failed. This VPN client certificate was rejected by the server.")
        );
    }

    #[cfg(unix)]
    #[test]
    fn stores_connected_addresses_and_clears_them_when_reconnecting() {
        let profile_id = ProfileId::new("vpn-addresses").expect("profile ID should be valid");
        let mut connection = crate::domain::VpnConnection::disconnected(profile_id);
        let tunnel_address: IpAddr = "10.8.0.2".parse().expect("tunnel IP should be valid");
        let remote_address: IpAddr = "198.51.100.10".parse().expect("remote IP should be valid");

        apply_state_update(
            &mut connection,
            ManagementStateUpdate {
                timestamp: 1_700_000_000,
                state: ConnectionState::Connected,
                description: Some("SUCCESS".to_owned()),
                tunnel_address: Some(tunnel_address),
                remote_address: Some(remote_address),
            },
        );

        assert_eq!(connection.tunnel_address, Some(tunnel_address));
        assert_eq!(connection.remote_address, Some(remote_address));

        apply_state_update(
            &mut connection,
            ManagementStateUpdate {
                timestamp: 1_700_000_001,
                state: ConnectionState::Reconnecting,
                description: None,
                tunnel_address: None,
                remote_address: None,
            },
        );

        assert_eq!(connection.tunnel_address, None);
        assert_eq!(connection.remote_address, None);
    }

    #[test]
    fn ignores_server_routes_when_strict_split_tunnel_is_enabled() {
        let profile_id =
            crate::domain::ProfileId::new("vpn-split").expect("profile ID should be valid");
        let mut connection = crate::domain::VpnConnection::disconnected(profile_id);
        let reply = crate::vpn::routing::parse_push_reply(
            "PUSH_REPLY,route 10.0.0.0 255.0.0.0,route 172.20.0.0 255.255.0.0",
        )
        .expect("push reply should parse");

        super::apply_management_event(
            &mut connection,
            crate::vpn::management::ManagementEvent::PushReply(reply.clone()),
            true,
        );
        assert!(connection.routes.is_empty());

        super::apply_management_event(
            &mut connection,
            crate::vpn::management::ManagementEvent::PushReply(reply),
            false,
        );
        assert_eq!(connection.routes.len(), 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn disconnecting_one_runtime_does_not_affect_another() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let manager = VpnManager::new();
        let mut updates = manager.subscribe();
        let (profile_a, process_a) = create_process(&workspace, "vpn-a").await;
        let (management_a, server_a) = create_management_client("signal SIGTERM").await;
        let port_a = management_a.port();
        manager
            .insert_runtime(
                profile_a.clone(),
                process_a,
                management_a,
                port_a,
                Vec::new(),
                None,
            )
            .await
            .expect("runtime A should be inserted");
        let initial_a = timeout(Duration::from_secs(2), updates.recv())
            .await
            .expect("initial runtime event should arrive")
            .expect("runtime event channel should remain open");
        assert_eq!(initial_a.profile_id, profile_a);
        assert_eq!(initial_a.state, ConnectionState::Connecting);

        let (profile_b, process_b) = create_process(&workspace, "vpn-b").await;
        let (management_b, server_b) = create_management_client("signal SIGTERM").await;
        let port_b = management_b.port();
        manager
            .insert_runtime(
                profile_b.clone(),
                process_b,
                management_b,
                port_b,
                Vec::new(),
                None,
            )
            .await
            .expect("runtime B should be inserted");

        wait_for_state(&manager, &profile_a, ConnectionState::Connected).await;
        wait_for_state(&manager, &profile_b, ConnectionState::Connected).await;
        wait_for_byte_count(&manager, &profile_a, 4096, 1024).await;
        wait_for_byte_count(&manager, &profile_b, 4096, 1024).await;
        let before_b = manager.status(&profile_b).await;
        assert_ne!(port_a, port_b);
        assert_eq!(manager.statuses().await.len(), 2);
        assert!(matches!(
            manager.ensure_not_active(&profile_a).await,
            Err(AppError::ConnectionAlreadyActive { .. })
        ));

        let stopped_a = manager
            .stop(&profile_a)
            .await
            .expect("runtime A should stop");
        assert_eq!(stopped_a.state, ConnectionState::Disconnected);
        assert_eq!(
            manager.status(&profile_a).await.state,
            ConnectionState::Disconnected
        );

        let after_b = manager.status(&profile_b).await;
        assert_eq!(after_b.state, ConnectionState::Connected);
        assert_eq!(after_b.process_id, before_b.process_id);
        assert_eq!(after_b.management_port, before_b.management_port);
        assert_eq!(manager.statuses().await.len(), 1);
        let stopped_a_again = manager
            .stop(&profile_a)
            .await
            .expect("duplicate disconnect should be idempotent");
        assert_eq!(stopped_a_again.state, ConnectionState::Disconnected);

        manager
            .shutdown_all()
            .await
            .expect("remaining runtimes should stop during shutdown");
        assert!(manager.statuses().await.is_empty());
        server_a.await.expect("mock server A should finish");
        server_b.await.expect("mock server B should finish");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_crash_sets_error_and_finished_runtime_can_be_replaced() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let manager = VpnManager::new();
        let (profile_id, process) =
            create_process_with_script(&workspace, "vpn-crash", "#!/bin/sh\nsleep 0.1\nexit 17\n")
                .await;
        let (management, crashed_server) = create_management_client("quit").await;
        let management_port = management.port();
        manager
            .insert_runtime(
                profile_id.clone(),
                process,
                management,
                management_port,
                Vec::new(),
                None,
            )
            .await
            .expect("crashing runtime should be inserted");

        wait_for_state(&manager, &profile_id, ConnectionState::Error).await;
        let crashed = manager.status(&profile_id).await;
        assert_eq!(crashed.process_id, None);
        assert_eq!(crashed.management_port, None);
        assert_eq!(crashed.connected_at, None);
        wait_for_runtime_finish(&manager, &profile_id).await;

        let (replacement_id, replacement_process) = create_process(&workspace, "vpn-crash").await;
        let (replacement_management, replacement_server) =
            create_management_client("signal SIGTERM").await;
        let replacement_port = replacement_management.port();
        manager
            .insert_runtime(
                replacement_id,
                replacement_process,
                replacement_management,
                replacement_port,
                Vec::new(),
                None,
            )
            .await
            .expect("finished runtime should be reaped before reconnecting");
        wait_for_state(&manager, &profile_id, ConnectionState::Connected).await;

        manager
            .stop(&profile_id)
            .await
            .expect("replacement runtime should stop");
        crashed_server
            .await
            .expect("crashed runtime server should finish");
        replacement_server
            .await
            .expect("replacement runtime server should finish");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn management_timeout_sets_error_and_stops_the_owned_process() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let manager = VpnManager::new();
        let (profile_id, process) = create_process(&workspace, "vpn-timeout").await;
        let (management, server) = create_silent_management_client().await;
        let management_port = management.port();
        manager
            .insert_runtime(
                profile_id.clone(),
                process,
                management,
                management_port,
                Vec::new(),
                None,
            )
            .await
            .expect("runtime should be inserted");

        wait_for_state(&manager, &profile_id, ConnectionState::Error).await;
        wait_for_runtime_finish(&manager, &profile_id).await;
        let failed = manager.status(&profile_id).await;
        assert_eq!(failed.process_id, None);
        assert_eq!(failed.management_port, None);
        assert_eq!(failed.connected_at, None);

        let recovered = manager
            .stop(&profile_id)
            .await
            .expect("failed runtime cleanup should be idempotent");
        assert_eq!(recovered.state, ConnectionState::Disconnected);
        server.await.expect("mock server should finish");
    }

    #[cfg(unix)]
    async fn create_process(workspace: &TempDir, id: &str) -> (ProfileId, OpenVpnProcess) {
        create_process_with_script(workspace, id, "#!/bin/sh\nwhile :; do sleep 1; done\n").await
    }

    #[cfg(unix)]
    async fn create_process_with_script(
        workspace: &TempDir,
        id: &str,
        script: &str,
    ) -> (ProfileId, OpenVpnProcess) {
        use std::os::unix::fs::PermissionsExt;

        let directory = workspace.path().join(id);
        fs::create_dir_all(&directory).expect("profile directory should be created");
        let executable = directory.join("fake-openvpn");
        let config = directory.join("config.ovpn");
        fs::write(&executable, script).expect("fake executable should be written");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .expect("fake executable permissions should be set");
        fs::write(&config, "client\n").expect("fake config should be written");
        let profile_id = ProfileId::new(id).expect("profile ID should be valid");
        let process = OpenVpnProcess::spawn_command(profile_id.clone(), Command::new(&executable))
            .await
            .expect("fake OpenVPN process should start");
        (profile_id, process)
    }

    #[cfg(unix)]
    async fn create_management_client(
        final_command: &'static str,
    ) -> (ManagementClient, JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("mock management listener should bind");
        let port = listener
            .local_addr()
            .expect("listener address should be available")
            .port();
        let server = tokio::spawn(async move {
            let (socket, _) = listener
                .accept()
                .await
                .expect("management client should connect");
            let (reader, mut writer) = socket.into_split();
            let mut commands = BufReader::new(reader).lines();
            assert_eq!(
                read_command(&mut commands).await.as_deref(),
                Some("state on")
            );
            assert_eq!(
                read_command(&mut commands).await.as_deref(),
                Some("bytecount 1")
            );
            assert_eq!(
                read_command(&mut commands).await.as_deref(),
                Some("log on all")
            );
            writer
                .write_all(b">STATE:1700000000,CONNECTED,SUCCESS,10.8.0.2,198.51.100.10\n")
                .await
                .expect("connected event should be written");
            writer
                .write_all(b">BYTECOUNT:4096,1024\n")
                .await
                .expect("bytecount event should be written");
            assert_eq!(
                read_command(&mut commands).await.as_deref(),
                Some(final_command)
            );
        });
        let client = ManagementClient::connect(port)
            .await
            .expect("management client should connect");
        (client, server)
    }

    #[cfg(unix)]
    async fn create_silent_management_client() -> (ManagementClient, JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("mock management listener should bind");
        let port = listener
            .local_addr()
            .expect("listener address should be available")
            .port();
        let server = tokio::spawn(async move {
            let (socket, _) = listener
                .accept()
                .await
                .expect("management client should connect");
            let (reader, mut writer) = socket.into_split();
            let mut commands = BufReader::new(reader).lines();
            for expected in ["state on", "bytecount 1", "log on all"] {
                assert_eq!(read_command(&mut commands).await.as_deref(), Some(expected));
            }
            writer
                .write_all(b">STATE:1700000000,CONNECTED,SUCCESS,10.8.0.2,198.51.100.10\n")
                .await
                .expect("connected event should be written");
            assert_eq!(
                read_command(&mut commands).await.as_deref(),
                Some("signal SIGTERM")
            );
        });
        let client = ManagementClient::connect_with_idle_timeout(port, Duration::from_millis(20))
            .await
            .expect("management client should connect");
        (client, server)
    }

    #[cfg(unix)]
    async fn read_command(
        commands: &mut tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
    ) -> Option<String> {
        timeout(Duration::from_secs(2), commands.next_line())
            .await
            .expect("management command should arrive")
            .expect("management command should be readable")
    }

    #[cfg(unix)]
    async fn wait_for_state(
        manager: &VpnManager,
        profile_id: &ProfileId,
        expected: ConnectionState,
    ) {
        timeout(Duration::from_secs(2), async {
            loop {
                if manager.status(profile_id).await.state == expected {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("runtime state should update");
    }

    #[cfg(unix)]
    async fn wait_for_byte_count(
        manager: &VpnManager,
        profile_id: &ProfileId,
        received: u64,
        sent: u64,
    ) {
        timeout(Duration::from_secs(2), async {
            loop {
                let connection = manager.status(profile_id).await;
                if connection.bytes_received == received && connection.bytes_sent == sent {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("runtime traffic should update");
    }

    #[cfg(unix)]
    async fn wait_for_runtime_finish(manager: &VpnManager, profile_id: &ProfileId) {
        timeout(Duration::from_secs(2), async {
            loop {
                let finished = manager
                    .runtimes
                    .read()
                    .await
                    .get(profile_id)
                    .is_some_and(|runtime| runtime.task.is_finished());
                if finished {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("runtime task should finish");
    }
}
