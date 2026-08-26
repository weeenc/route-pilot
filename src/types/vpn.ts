export type ConnectionState =
  | "disconnected"
  | "connecting"
  | "connected"
  | "reconnecting"
  | "disconnecting"
  | "error";

export type RouteSource = "serverPush" | "config" | "runtime" | "system";

export interface VpnRoute {
  network: string;
  gateway: string | null;
  interface: string | null;
  source: RouteSource;
}

export interface RouteConflict {
  leftProfileId: string;
  leftNetwork: string;
  rightProfileId: string;
  rightNetwork: string;
}

export interface VpnConnection {
  profileId: string;
  state: ConnectionState;
  processId: number | null;
  managementPort: number | null;
  connectedAt: string | null;
  errorMessage: string | null;
  bytesReceived: number;
  bytesSent: number;
  tunnelAddress: string | null;
  remoteAddress: string | null;
  tunnelInterface: string | null;
  routes: VpnRoute[];
}

export type ConnectionAction = "connect" | "disconnect";
