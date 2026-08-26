use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddrV4},
    time::Duration,
};

use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf, TcpStream},
    sync::mpsc,
    task::JoinHandle,
    time::timeout,
};

use crate::{domain::ConnectionState, error::AppError};

use super::routing::{parse_push_reply, PushReply};

#[cfg(any(not(target_os = "macos"), test))]
pub(super) const MANAGEMENT_HOST: &str = "127.0.0.1";
const STATE_COMMAND: &str = "state on";
const BYTECOUNT_COMMAND: &str = "bytecount 1";
const LOG_COMMAND: &str = "log on all";
const QUIT_COMMAND: &str = "quit";
const TERMINATE_COMMAND: &str = "signal SIGTERM";
const EVENT_CHANNEL_CAPACITY: usize = 256;
const MAX_DESCRIPTION_CHARACTERS: usize = 1024;
const SOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const CONNECTED_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// A normalized OpenVPN state notification.
///
/// Raw OpenVPN state names are intentionally mapped to the RoutePilot domain
/// model here so callers never need to interpret Management Interface strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagementStateUpdate {
    pub timestamp: i64,
    pub state: ConnectionState,
    pub description: Option<String>,
    pub tunnel_address: Option<IpAddr>,
    pub remote_address: Option<IpAddr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteCount {
    pub bytes_received: u64,
    pub bytes_sent: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagementEvent {
    State(ManagementStateUpdate),
    ByteCount(ByteCount),
    PushReply(PushReply),
}

/// Parser failures never contain the original management line because it may
/// include connection or authentication-related information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ManagementParseError {
    #[error("state notification is missing a required field")]
    MissingStateField,
    #[error("state notification has an invalid timestamp")]
    InvalidStateTimestamp,
    #[error("bytecount notification is malformed")]
    InvalidByteCount,
}

/// One TCP client for one OpenVPN Management Interface.
///
/// The endpoint is always IPv4 loopback. Accepting only a port prevents callers
/// from accidentally exposing or connecting the cleartext protocol elsewhere.
pub struct ManagementClient {
    port: u16,
    writer: OwnedWriteHalf,
    events: mpsc::Receiver<Result<ManagementEvent, AppError>>,
    reader_task: JoinHandle<()>,
}

impl ManagementClient {
    pub async fn connect(port: u16) -> Result<Self, AppError> {
        Self::connect_with_idle_timeout(port, CONNECTED_IDLE_TIMEOUT).await
    }

    pub(super) async fn connect_with_idle_timeout(
        port: u16,
        idle_timeout: Duration,
    ) -> Result<Self, AppError> {
        if port == 0 {
            return Err(AppError::ManagementConnectFailed {
                reason: "management port cannot be zero".to_owned(),
            });
        }

        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let stream = timeout(SOCKET_CONNECT_TIMEOUT, TcpStream::connect(address))
            .await
            .map_err(|_| AppError::ManagementTimeout)?
            .map_err(connect_error)?;
        Self::from_stream_with_idle_timeout(stream, port, idle_timeout).await
    }

    /// Builds a management client from a connection accepted by RoutePilot.
    ///
    /// Production launches use OpenVPN's `--management-client` mode so RoutePilot
    /// owns the listening socket before the privileged process starts. This keeps
    /// the cleartext management protocol from being exposed as a local TCP server
    /// and removes the release-then-bind port race.
    pub(super) async fn from_stream(stream: TcpStream, port: u16) -> Result<Self, AppError> {
        Self::from_stream_with_idle_timeout(stream, port, CONNECTED_IDLE_TIMEOUT).await
    }

    async fn from_stream_with_idle_timeout(
        stream: TcpStream,
        port: u16,
        idle_timeout: Duration,
    ) -> Result<Self, AppError> {
        if port == 0
            || !stream
                .peer_addr()
                .map_err(connect_error)?
                .ip()
                .is_loopback()
        {
            return Err(AppError::ManagementConnectFailed {
                reason: "management connection must use IPv4 loopback".to_owned(),
            });
        }
        let (reader, writer) = stream.into_split();
        let (event_sender, events) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let reader_task = tokio::spawn(read_management_events(reader, event_sender, idle_timeout));

        let mut client = Self {
            port,
            writer,
            events,
            reader_task,
        };
        if let Err(error) = client.enable_events().await {
            client.reader_task.abort();
            return Err(error);
        }

        Ok(client)
    }

    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Waits for the next normalized state or traffic notification.
    /// `None` means the Management Interface closed the connection cleanly.
    pub async fn next_event(&mut self) -> Result<Option<ManagementEvent>, AppError> {
        match self.events.recv().await {
            Some(event) => event.map(Some),
            None => Ok(None),
        }
    }

    /// Closes this management session without terminating the OpenVPN process.
    pub async fn disconnect(mut self) -> Result<(), AppError> {
        let command_result = self.write_command(QUIT_COMMAND).await;
        let shutdown_result = self.writer.shutdown().await.map_err(connect_error);
        self.reader_task.abort();

        command_result.and(shutdown_result)
    }

    /// Requests that OpenVPN terminate before closing the Management Interface.
    ///
    /// This is required for privileged macOS launches: killing the unprivileged
    /// launcher does not reliably terminate the root-owned OpenVPN process.
    pub async fn terminate(mut self) -> Result<(), AppError> {
        let command_result = self.write_command(TERMINATE_COMMAND).await;
        let shutdown_result = self.writer.shutdown().await.map_err(connect_error);
        self.reader_task.abort();

        command_result.and(shutdown_result)
    }

    async fn enable_events(&mut self) -> Result<(), AppError> {
        self.write_command(STATE_COMMAND).await?;
        self.write_command(BYTECOUNT_COMMAND).await?;
        self.write_command(LOG_COMMAND).await
    }

    async fn write_command(&mut self, command: &'static str) -> Result<(), AppError> {
        self.writer
            .write_all(command.as_bytes())
            .await
            .map_err(connect_error)?;
        self.writer.write_all(b"\n").await.map_err(connect_error)?;
        self.writer.flush().await.map_err(connect_error)
    }
}

impl Drop for ManagementClient {
    fn drop(&mut self) {
        self.reader_task.abort();
    }
}

pub fn parse_management_event(line: &str) -> Result<Option<ManagementEvent>, ManagementParseError> {
    let line = line.trim_end_matches(['\r', '\n']);
    if let Some(payload) = line.strip_prefix(">STATE:") {
        return parse_state(payload).map(|state| Some(ManagementEvent::State(state)));
    }
    if let Some(payload) = line.strip_prefix(">BYTECOUNT:") {
        return parse_byte_count(payload).map(|count| Some(ManagementEvent::ByteCount(count)));
    }
    if line.starts_with(">LOG:") {
        return Ok(parse_push_reply(line).map(ManagementEvent::PushReply));
    }

    Ok(None)
}

fn parse_state(payload: &str) -> Result<ManagementStateUpdate, ManagementParseError> {
    let mut fields = payload.split(',');
    let timestamp = fields
        .next()
        .ok_or(ManagementParseError::MissingStateField)?
        .parse::<i64>()
        .map_err(|_| ManagementParseError::InvalidStateTimestamp)?;
    let raw_state = fields
        .next()
        .filter(|state| !state.is_empty())
        .ok_or(ManagementParseError::MissingStateField)?;

    Ok(ManagementStateUpdate {
        timestamp,
        state: map_connection_state(raw_state),
        description: fields.next().and_then(sanitized_optional_text),
        tunnel_address: fields.next().and_then(parse_optional_ip),
        remote_address: fields.next().and_then(parse_optional_ip),
    })
}

fn parse_byte_count(payload: &str) -> Result<ByteCount, ManagementParseError> {
    let mut fields = payload.split(',');
    let bytes_received = parse_bytes(fields.next())?;
    let bytes_sent = parse_bytes(fields.next())?;
    if fields.next().is_some() {
        return Err(ManagementParseError::InvalidByteCount);
    }

    Ok(ByteCount {
        bytes_received,
        bytes_sent,
    })
}

fn parse_bytes(value: Option<&str>) -> Result<u64, ManagementParseError> {
    value
        .filter(|value| !value.is_empty())
        .ok_or(ManagementParseError::InvalidByteCount)?
        .parse::<u64>()
        .map_err(|_| ManagementParseError::InvalidByteCount)
}

fn map_connection_state(raw_state: &str) -> ConnectionState {
    match raw_state {
        "CONNECTED" => ConnectionState::Connected,
        "RECONNECTING" => ConnectionState::Reconnecting,
        "EXITING" => ConnectionState::Disconnecting,
        "DISCONNECTED" => ConnectionState::Disconnected,
        "CONNECTING" | "WAIT" | "AUTH" | "GET_CONFIG" | "ASSIGN_IP" | "ADD_ROUTES" | "RESOLVE"
        | "TCP_CONNECT" | "AUTH_PENDING" => ConnectionState::Connecting,
        _ => ConnectionState::Error,
    }
}

fn sanitized_optional_text(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }

    Some(
        value
            .chars()
            .filter(|character| !character.is_control() || *character == '\t')
            .take(MAX_DESCRIPTION_CHARACTERS)
            .collect(),
    )
}

fn parse_optional_ip(value: &str) -> Option<IpAddr> {
    if value.is_empty() {
        return None;
    }
    value.parse().ok()
}

async fn read_management_events(
    reader: OwnedReadHalf,
    sender: mpsc::Sender<Result<ManagementEvent, AppError>>,
    connected_idle_timeout: Duration,
) {
    let mut lines = BufReader::new(reader).lines();
    let mut connected = false;
    loop {
        let next_line = if connected {
            match timeout(connected_idle_timeout, lines.next_line()).await {
                Ok(result) => result.map_err(connect_error),
                Err(_) => Err(AppError::ManagementTimeout),
            }
        } else {
            lines.next_line().await.map_err(connect_error)
        };

        match next_line {
            Ok(Some(line)) => match parse_management_event(&line) {
                Ok(Some(event)) => {
                    if let ManagementEvent::State(update) = &event {
                        connected = update.state == ConnectionState::Connected;
                    }
                    if sender.send(Ok(event)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    let error = AppError::ManagementProtocolInvalid {
                        reason: error.to_string(),
                    };
                    if sender.send(Err(error)).await.is_err() {
                        break;
                    }
                }
            },
            Ok(None) => break,
            Err(error) => {
                let _ = sender.send(Err(error)).await;
                break;
            }
        }
    }
}

fn connect_error(error: io::Error) -> AppError {
    AppError::ManagementConnectFailed {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
        time::{sleep, timeout, Duration},
    };

    use crate::{domain::ConnectionState, error::AppError};

    use super::{
        parse_management_event, ByteCount, ManagementClient, ManagementEvent, ManagementParseError,
        ManagementStateUpdate,
    };

    #[test]
    fn parses_and_normalizes_all_required_openvpn_states() {
        let cases = [
            ("CONNECTING", ConnectionState::Connecting),
            ("AUTH", ConnectionState::Connecting),
            ("GET_CONFIG", ConnectionState::Connecting),
            ("ASSIGN_IP", ConnectionState::Connecting),
            ("ADD_ROUTES", ConnectionState::Connecting),
            ("CONNECTED", ConnectionState::Connected),
            ("RECONNECTING", ConnectionState::Reconnecting),
            ("EXITING", ConnectionState::Disconnecting),
        ];

        for (raw_state, expected) in cases {
            let line = format!(">STATE:1700000000,{raw_state},SUCCESS,10.8.0.2,198.51.100.10");
            let event = parse_management_event(&line)
                .expect("state should parse")
                .expect("state event should be recognized");
            let ManagementEvent::State(update) = event else {
                panic!("expected state event");
            };

            assert_eq!(update.state, expected);
            assert_eq!(update.timestamp, 1_700_000_000);
            assert_eq!(update.tunnel_address, "10.8.0.2".parse().ok());
            assert_eq!(update.remote_address, "198.51.100.10".parse().ok());
        }
    }

    #[test]
    fn parses_byte_count_using_client_receive_send_order() {
        let event = parse_management_event(">BYTECOUNT:123456,98765\r\n")
            .expect("bytecount should parse")
            .expect("bytecount event should be recognized");

        assert_eq!(
            event,
            ManagementEvent::ByteCount(ByteCount {
                bytes_received: 123_456,
                bytes_sent: 98_765,
            })
        );
    }

    #[test]
    fn rejects_malformed_relevant_notifications_and_ignores_responses() {
        assert_eq!(
            parse_management_event(">STATE:not-a-time,CONNECTED,,,")
                .expect_err("invalid timestamp should be rejected"),
            ManagementParseError::InvalidStateTimestamp
        );
        assert_eq!(
            parse_management_event(">BYTECOUNT:12,-3")
                .expect_err("negative bytecount should be rejected"),
            ManagementParseError::InvalidByteCount
        );
        assert_eq!(
            parse_management_event("SUCCESS: real-time state notification set to ON")
                .expect("command response should not fail"),
            None
        );
        assert_eq!(
            parse_management_event(">PASSWORD:Need 'Auth' username/password")
                .expect("unsupported event should be ignored"),
            None
        );
    }

    #[tokio::test]
    async fn connects_on_loopback_enables_events_and_receives_updates() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("mock management listener should bind");
        let port = listener
            .local_addr()
            .expect("listener address should be available")
            .port();
        let server = tokio::spawn(async move {
            let (socket, peer) = listener
                .accept()
                .await
                .expect("management client should connect");
            assert_eq!(peer.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
            let (reader, mut writer) = socket.into_split();
            let mut commands = BufReader::new(reader).lines();

            writer
                .write_all(
                    b">INFO:OpenVPN Management Interface Version 5\r\n\
SUCCESS: ready\r\n",
                )
                .await
                .expect("mock greeting should be written");
            assert_eq!(
                commands
                    .next_line()
                    .await
                    .expect("state command should be readable")
                    .as_deref(),
                Some("state on")
            );
            assert_eq!(
                commands
                    .next_line()
                    .await
                    .expect("bytecount command should be readable")
                    .as_deref(),
                Some("bytecount 1")
            );
            assert_eq!(
                commands
                    .next_line()
                    .await
                    .expect("log command should be readable")
                    .as_deref(),
                Some("log on all")
            );

            writer
                .write_all(
                    b"SUCCESS: real-time state notification set to ON\r\n\
>STATE:1700000000,CONNECTED,SUCCESS,10.8.0.2,198.51.100.10\r\n\
SUCCESS: bytecount interval changed\r\n\
>BYTECOUNT:4096,1024\r\n",
                )
                .await
                .expect("mock events should be written");

            assert_eq!(
                commands
                    .next_line()
                    .await
                    .expect("quit command should be readable")
                    .as_deref(),
                Some("quit")
            );
        });

        let mut client = ManagementClient::connect(port)
            .await
            .expect("management client should connect");
        assert_eq!(client.port(), port);

        let state = timeout(Duration::from_secs(1), client.next_event())
            .await
            .expect("state event should arrive")
            .expect("state read should succeed")
            .expect("management connection should remain open");
        assert_eq!(
            state,
            ManagementEvent::State(ManagementStateUpdate {
                timestamp: 1_700_000_000,
                state: ConnectionState::Connected,
                description: Some("SUCCESS".to_owned()),
                tunnel_address: "10.8.0.2".parse().ok(),
                remote_address: "198.51.100.10".parse().ok(),
            })
        );

        let byte_count = timeout(Duration::from_secs(1), client.next_event())
            .await
            .expect("bytecount event should arrive")
            .expect("bytecount read should succeed")
            .expect("management connection should remain open");
        assert_eq!(
            byte_count,
            ManagementEvent::ByteCount(ByteCount {
                bytes_received: 4096,
                bytes_sent: 1024,
            })
        );

        client
            .disconnect()
            .await
            .expect("management session should disconnect");
        server.await.expect("mock server should finish");
    }

    #[tokio::test]
    async fn rejects_zero_port_without_opening_a_socket() {
        let result = ManagementClient::connect(0).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn times_out_a_silent_management_channel_after_connected_state() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("mock management listener should bind");
        let port = listener.local_addr().expect("address should exist").port();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("client should connect");
            let (reader, mut writer) = socket.into_split();
            let mut commands = BufReader::new(reader).lines();
            for expected in ["state on", "bytecount 1", "log on all"] {
                assert_eq!(
                    commands
                        .next_line()
                        .await
                        .expect("command should be readable")
                        .as_deref(),
                    Some(expected)
                );
            }
            writer
                .write_all(b">STATE:1700000000,CONNECTED,SUCCESS,10.8.0.2,198.51.100.10\r\n")
                .await
                .expect("connected event should be written");
            sleep(Duration::from_millis(100)).await;
        });

        let mut client =
            ManagementClient::connect_with_idle_timeout(port, Duration::from_millis(20))
                .await
                .expect("management client should connect");
        assert!(matches!(
            client.next_event().await,
            Ok(Some(ManagementEvent::State(_)))
        ));
        assert!(matches!(
            client.next_event().await,
            Err(AppError::ManagementTimeout)
        ));

        server.await.expect("mock server should finish");
    }

    #[test]
    fn parses_push_reply_routes_from_management_logs() {
        let event = parse_management_event(
            ">LOG:1700000000,I,PUSH: Received control message: 'PUSH_REPLY,route 10.0.0.0 255.0.0.0,redirect-gateway def1'",
        )
        .expect("management log should parse")
        .expect("push reply should be recognized");

        let ManagementEvent::PushReply(reply) = event else {
            panic!("expected push reply event");
        };
        assert_eq!(reply.routes[0].network.to_string(), "10.0.0.0/8");
        assert!(reply.requested_redirect_gateway);
    }
}
