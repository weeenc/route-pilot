export interface VpnProfile {
  id: string;
  name: string;
  configPath: string;
  serverHost: string | null;
  serverPort: number | null;
  protocol: string | null;
  autoReconnect: boolean;
  autoConnect: boolean;
  ignoreRedirectGateway: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface UpdateVpnProfileInput {
  name: string;
  ignoreRedirectGateway: boolean;
}
