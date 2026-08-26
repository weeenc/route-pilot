use std::net::IpAddr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{ConnectionState, ProfileId, Route};

/// Runtime state for exactly one OpenVPN process and Management Interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnConnection {
    pub profile_id: ProfileId,
    pub state: ConnectionState,
    pub process_id: Option<u32>,
    pub management_port: Option<u16>,
    pub connected_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub tunnel_address: Option<IpAddr>,
    pub remote_address: Option<IpAddr>,
    pub tunnel_interface: Option<String>,
    pub routes: Vec<Route>,
}

impl VpnConnection {
    #[must_use]
    pub fn disconnected(profile_id: ProfileId) -> Self {
        Self {
            profile_id,
            state: ConnectionState::Disconnected,
            process_id: None,
            management_port: None,
            connected_at: None,
            error_message: None,
            bytes_received: 0,
            bytes_sent: 0,
            tunnel_address: None,
            remote_address: None,
            tunnel_interface: None,
            routes: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VpnConnection;
    use crate::domain::{ConnectionState, ProfileId};

    #[test]
    fn initializes_runtime_independently_for_each_profile() {
        let connection = VpnConnection::disconnected(
            ProfileId::new("vpn-a").expect("profile ID should be valid"),
        );

        assert_eq!(connection.state, ConnectionState::Disconnected);
        assert_eq!(connection.process_id, None);
        assert_eq!(connection.management_port, None);
        assert_eq!(connection.error_message, None);
        assert_eq!(connection.bytes_received, 0);
        assert_eq!(connection.bytes_sent, 0);
        assert_eq!(connection.tunnel_address, None);
        assert_eq!(connection.remote_address, None);
        assert!(connection.routes.is_empty());
    }

    #[test]
    fn serializes_runtime_fields_as_camel_case() {
        let connection = VpnConnection::disconnected(
            ProfileId::new("vpn-a").expect("profile ID should be valid"),
        );

        let value = serde_json::to_value(connection).expect("connection should serialize");

        assert_eq!(value["profileId"], "vpn-a");
        assert_eq!(value["state"], "disconnected");
        assert_eq!(value["bytesReceived"], 0);
        assert!(value["errorMessage"].is_null());
        assert_eq!(value["bytesSent"], 0);
        assert!(value["tunnelAddress"].is_null());
        assert!(value["remoteAddress"].is_null());
        assert!(value.get("profile_id").is_none());
    }
}
