import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { RouteConflict, VpnConnection } from "../types/vpn";

export const CONNECTION_UPDATED_EVENT = "vpn://connection-updated";

export function connectProfile(profileId: string): Promise<VpnConnection> {
  return invoke<VpnConnection>("connect_profile", { profileId });
}

export function disconnectProfile(profileId: string): Promise<VpnConnection> {
  return invoke<VpnConnection>("disconnect_profile", { profileId });
}

export function getConnection(profileId: string): Promise<VpnConnection> {
  return invoke<VpnConnection>("get_connection", { profileId });
}

export function listConnections(): Promise<VpnConnection[]> {
  return invoke<VpnConnection[]>("list_connections");
}

export function listRouteConflicts(): Promise<RouteConflict[]> {
  return invoke<RouteConflict[]>("list_route_conflicts");
}

export function listenForConnectionUpdates(
  handler: (connection: VpnConnection) => void,
): Promise<UnlistenFn> {
  return listen<VpnConnection>(CONNECTION_UPDATED_EVENT, (event) => handler(event.payload));
}
