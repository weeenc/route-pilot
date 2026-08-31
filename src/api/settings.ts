import { invoke, isTauri } from "@tauri-apps/api/core";

import type {
  AppSettings,
  LocatedOpenVpn,
  PrivilegedHelperStatus,
} from "../types/settings";

export const isDesktopRuntime = isTauri();

export function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export function setOpenVpnExecutable(path: string | null): Promise<AppSettings> {
  return invoke<AppSettings>("set_openvpn_executable", { path });
}

export function setCheckForUpdatesOnStartup(enabled: boolean): Promise<AppSettings> {
  return invoke<AppSettings>("set_check_for_updates_on_startup", { enabled });
}

export function locateOpenVpn(): Promise<LocatedOpenVpn> {
  return invoke<LocatedOpenVpn>("locate_openvpn");
}

export function getPrivilegedHelperStatus(): Promise<PrivilegedHelperStatus> {
  return invoke<PrivilegedHelperStatus>("get_privileged_helper_status");
}

export function enablePrivilegedHelper(): Promise<PrivilegedHelperStatus> {
  return invoke<PrivilegedHelperStatus>("enable_privileged_helper");
}
