export type OpenVpnSource = "bundled" | "custom" | "path" | "common";

export interface AppSettings {
  openvpnExecutable: string | null;
}

export interface LocatedOpenVpn {
  path: string;
  source: OpenVpnSource;
}

export type PrivilegedHelperState =
  | "installed"
  | "notInstalled"
  | "unavailable"
  | "outdated"
  | "unsupported";

export interface PrivilegedHelperStatus {
  state: PrivilegedHelperState;
  installedVersion: number | null;
  expectedVersion: number;
}
