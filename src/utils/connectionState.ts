import type { ConnectionState } from "../types/vpn";

const connectedStates: ReadonlySet<ConnectionState> = new Set(["connected", "reconnecting"]);
const connectingStates: ReadonlySet<ConnectionState> = new Set(["connecting", "reconnecting"]);
const disconnectableStates: ReadonlySet<ConnectionState> = new Set([
  "connecting",
  "connected",
  "reconnecting",
  "disconnecting",
]);

export function isConnectedState(state: ConnectionState): boolean {
  return connectedStates.has(state);
}

export function isConnectingState(state: ConnectionState): boolean {
  return connectingStates.has(state);
}

export function canDisconnectState(state: ConnectionState): boolean {
  return disconnectableStates.has(state);
}
