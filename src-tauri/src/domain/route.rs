use std::net::IpAddr;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

use super::{ConnectionState, ProfileId, VpnConnection};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteSource {
    ServerPush,
    Config,
    Runtime,
    System,
}

/// A validated IP network route associated with a VPN connection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub network: IpNet,
    pub gateway: Option<IpAddr>,
    pub interface: Option<String>,
    pub source: RouteSource,
}

/// One pair of routes owned by different active VPNs whose networks overlap.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteConflict {
    pub left_profile_id: ProfileId,
    pub left_network: IpNet,
    pub right_profile_id: ProfileId,
    pub right_network: IpNet,
}

impl Route {
    pub fn new(
        network: IpAddr,
        prefix: u8,
        gateway: Option<IpAddr>,
        interface: Option<String>,
        source: RouteSource,
    ) -> Result<Self, AppError> {
        let network = IpNet::new(network, prefix)
            .map_err(|error| AppError::ConfigInvalid {
                reason: format!("invalid route prefix: {error}"),
            })?
            .trunc();

        if let Some(gateway) = gateway {
            if network.addr().is_ipv4() != gateway.is_ipv4() {
                return Err(AppError::ConfigInvalid {
                    reason: "route network and gateway use different IP families".to_owned(),
                });
            }
        }

        Ok(Self {
            network,
            gateway,
            interface,
            source,
        })
    }
}

/// Finds overlaps across active VPNs using normalized IP networks.
///
/// Routes belonging to the same profile and IPv4/IPv6 pairs are intentionally
/// excluded. Results are deterministic and duplicate network pairs are folded.
#[must_use]
pub fn detect_route_conflicts(connections: &[VpnConnection]) -> Vec<RouteConflict> {
    let mut active = connections
        .iter()
        .filter(|connection| {
            matches!(
                connection.state,
                ConnectionState::Connecting
                    | ConnectionState::Connected
                    | ConnectionState::Reconnecting
                    | ConnectionState::Disconnecting
            )
        })
        .collect::<Vec<_>>();
    active.sort_by(|left, right| left.profile_id.as_str().cmp(right.profile_id.as_str()));

    let mut conflicts = Vec::new();
    for (left_index, left) in active.iter().enumerate() {
        for right in active.iter().skip(left_index + 1) {
            for left_route in &left.routes {
                for right_route in &right.routes {
                    if networks_overlap(&left_route.network, &right_route.network) {
                        conflicts.push(RouteConflict {
                            left_profile_id: left.profile_id.clone(),
                            left_network: left_route.network,
                            right_profile_id: right.profile_id.clone(),
                            right_network: right_route.network,
                        });
                    }
                }
            }
        }
    }

    conflicts.sort_by(|left, right| {
        (
            left.left_profile_id.as_str(),
            left.left_network.to_string(),
            left.right_profile_id.as_str(),
            left.right_network.to_string(),
        )
            .cmp(&(
                right.left_profile_id.as_str(),
                right.left_network.to_string(),
                right.right_profile_id.as_str(),
                right.right_network.to_string(),
            ))
    });
    conflicts.dedup();
    conflicts
}

fn networks_overlap(left: &IpNet, right: &IpNet) -> bool {
    if left.addr().is_ipv4() != right.addr().is_ipv4() {
        return false;
    }

    left.contains(&right.network()) || right.contains(&left.network())
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::{detect_route_conflicts, Route, RouteSource};
    use crate::domain::{ConnectionState, ProfileId, VpnConnection};

    #[test]
    fn normalizes_network_address_and_serializes_source() {
        let route = Route::new(
            IpAddr::V4(Ipv4Addr::new(10, 10, 5, 24)),
            16,
            None,
            Some("utun5".to_owned()),
            RouteSource::ServerPush,
        )
        .expect("route should be valid");

        assert_eq!(route.network.to_string(), "10.10.0.0/16");

        let value = serde_json::to_value(route).expect("route should serialize");
        assert_eq!(value["network"], "10.10.0.0/16");
        assert_eq!(value["source"], "serverPush");
    }

    #[test]
    fn rejects_invalid_prefixes() {
        let result = Route::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            33,
            None,
            None,
            RouteSource::Config,
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_mixed_network_and_gateway_families() {
        let result = Route::new(
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)),
            8,
            Some(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            None,
            RouteSource::Runtime,
        );

        assert!(result.is_err());
    }

    #[test]
    fn detects_overlaps_only_across_active_vpns_of_the_same_ip_family() {
        let mut vpn_a = active_connection("vpn-a");
        vpn_a.routes.push(route("10.0.0.0", 8));
        vpn_a.routes.push(route("fd00::", 8));

        let mut vpn_b = active_connection("vpn-b");
        vpn_b.routes.push(route("10.10.0.0", 16));
        vpn_b.routes.push(route("172.20.0.0", 16));
        vpn_b.routes.push(route("2001:db8::", 32));

        let conflicts = detect_route_conflicts(&[vpn_b, vpn_a]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].left_profile_id.as_str(), "vpn-a");
        assert_eq!(conflicts[0].left_network.to_string(), "10.0.0.0/8");
        assert_eq!(conflicts[0].right_profile_id.as_str(), "vpn-b");
        assert_eq!(conflicts[0].right_network.to_string(), "10.10.0.0/16");
    }

    #[test]
    fn ignores_same_profile_duplicates_and_inactive_connections() {
        let mut disconnected = VpnConnection::disconnected(
            ProfileId::new("vpn-b").expect("profile ID should be valid"),
        );
        disconnected.routes.push(route("10.10.0.0", 16));

        let mut active = active_connection("vpn-a");
        active.routes.push(route("10.0.0.0", 8));
        active.routes.push(route("10.10.0.0", 16));

        assert!(detect_route_conflicts(&[active, disconnected]).is_empty());
    }

    fn active_connection(id: &str) -> VpnConnection {
        let mut connection =
            VpnConnection::disconnected(ProfileId::new(id).expect("profile ID should be valid"));
        connection.state = ConnectionState::Connected;
        connection
    }

    fn route(address: &str, prefix: u8) -> Route {
        Route::new(
            address.parse().expect("route address should be valid"),
            prefix,
            None,
            None,
            RouteSource::ServerPush,
        )
        .expect("route should be valid")
    }
}
