use serde::{Deserialize, Serialize};

/// Application-level connection state. Raw OpenVPN state strings are mapped to
/// this enum by the Management Interface layer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Disconnecting,
    Error,
}

#[cfg(test)]
mod tests {
    use super::ConnectionState;

    #[test]
    fn defaults_to_disconnected() {
        assert_eq!(ConnectionState::default(), ConnectionState::Disconnected);
    }

    #[test]
    fn serializes_as_frontend_friendly_values() {
        let value = serde_json::to_string(&ConnectionState::Reconnecting)
            .expect("connection state should serialize");

        assert_eq!(value, "\"reconnecting\"");
    }
}
