import { defineStore } from "pinia";
import { ref } from "vue";

import { normalizeAppError } from "../api/errors";
import {
  deleteProfile as deleteProfileApi,
  importProfile as importProfileApi,
  listProfiles,
  selectProfilePath,
  updateProfile as updateProfileApi,
} from "../api/profile";
import { isDesktopRuntime } from "../api/settings";
import { translate } from "../i18n";
import type { UpdateVpnProfileInput, VpnProfile } from "../types/profile";

export const useProfilesStore = defineStore("profiles", () => {
  const profiles = ref<VpnProfile[]>([]);
  const isLoading = ref(false);
  const isImporting = ref(false);
  const updatingProfileIds = ref<Record<string, boolean | undefined>>({});
  const deletingProfileIds = ref<Record<string, boolean | undefined>>({});
  const errorMessage = ref("");

  async function load(): Promise<void> {
    errorMessage.value = "";
    if (!isDesktopRuntime) {
      profiles.value = [];
      return;
    }

    isLoading.value = true;
    try {
      profiles.value = await listProfiles();
    } catch (error: unknown) {
      errorMessage.value = normalizeAppError(error).message;
    } finally {
      isLoading.value = false;
    }
  }

  async function chooseAndImport(): Promise<VpnProfile | null> {
    errorMessage.value = "";
    if (!isDesktopRuntime) {
      errorMessage.value = translate("errors.profileImportDesktopOnly");
      return null;
    }

    isImporting.value = true;
    try {
      const sourcePath = await selectProfilePath();
      if (!sourcePath) {
        return null;
      }

      const profile = await importProfileApi(sourcePath);
      profiles.value = [...profiles.value, profile].sort((left, right) =>
        left.createdAt.localeCompare(right.createdAt),
      );
      return profile;
    } catch (error: unknown) {
      errorMessage.value = normalizeAppError(error).message;
      return null;
    } finally {
      isImporting.value = false;
    }
  }

  async function update(
    profileId: string,
    input: UpdateVpnProfileInput,
  ): Promise<boolean> {
    if (!isDesktopRuntime || updatingProfileIds.value[profileId]) return false;

    errorMessage.value = "";
    updatingProfileIds.value[profileId] = true;
    try {
      const updatedProfile = await updateProfileApi(profileId, input);
      profiles.value = profiles.value.map((profile) =>
        profile.id === profileId ? updatedProfile : profile,
      );
      return true;
    } catch (error: unknown) {
      errorMessage.value = normalizeAppError(error).message;
      return false;
    } finally {
      updatingProfileIds.value[profileId] = undefined;
    }
  }

  async function remove(profileId: string): Promise<boolean> {
    if (!isDesktopRuntime || deletingProfileIds.value[profileId]) return false;

    errorMessage.value = "";
    deletingProfileIds.value[profileId] = true;
    try {
      await deleteProfileApi(profileId);
      profiles.value = profiles.value.filter((profile) => profile.id !== profileId);
      return true;
    } catch (error: unknown) {
      errorMessage.value = normalizeAppError(error).message;
      return false;
    } finally {
      deletingProfileIds.value[profileId] = undefined;
    }
  }

  return {
    profiles,
    isLoading,
    isImporting,
    updatingProfileIds,
    deletingProfileIds,
    errorMessage,
    load,
    chooseAndImport,
    update,
    remove,
  };
});
