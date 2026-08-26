import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import { translate } from "../i18n";
import type { UpdateVpnProfileInput, VpnProfile } from "../types/profile";

export function listProfiles(): Promise<VpnProfile[]> {
  return invoke<VpnProfile[]>("list_profiles");
}

export function importProfile(sourcePath: string): Promise<VpnProfile> {
  return invoke<VpnProfile>("import_profile", { sourcePath });
}

export function updateProfile(
  profileId: string,
  input: UpdateVpnProfileInput,
): Promise<VpnProfile> {
  return invoke<VpnProfile>("update_profile", { profileId, input });
}

export function deleteProfile(profileId: string): Promise<void> {
  return invoke<void>("delete_profile", { profileId });
}

export function selectProfilePath(): Promise<string | null> {
  return open({
    multiple: false,
    directory: false,
    filters: [{ name: translate("dialogs.openVpnProfile"), extensions: ["ovpn"] }],
  });
}
