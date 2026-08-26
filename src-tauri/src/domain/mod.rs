//! Core RoutePilot domain models.

mod connection;
mod profile;
mod route;
mod state;

pub use connection::VpnConnection;
pub use profile::{ProfileId, VpnProfile};
pub use route::{detect_route_conflicts, Route, RouteConflict, RouteSource};
pub use state::ConnectionState;
