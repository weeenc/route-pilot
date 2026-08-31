import { defineStore } from "pinia";
import { computed, ref } from "vue";

import {
  checkForUpdate,
  openReleasePage,
  type ReleaseInfo,
} from "../api/update";

export type UpdateCheckStatus =
  | "idle"
  | "checking"
  | "current"
  | "available"
  | "error";

export const useUpdateStore = defineStore("update", () => {
  const status = ref<UpdateCheckStatus>("idle");
  const latestRelease = ref<ReleaseInfo | null>(null);
  const promptDismissed = ref(false);
  let activeCheck: Promise<void> | null = null;

  const shouldShowPrompt = computed(
    () => status.value === "available" && !promptDismissed.value,
  );

  function checkForUpdates(revealPrompt = false): Promise<void> {
    if (activeCheck) return activeCheck;

    status.value = "checking";
    activeCheck = checkForUpdate()
      .then((release) => {
        latestRelease.value = release;
        status.value = release ? "available" : "current";
        if (release && revealPrompt) promptDismissed.value = false;
      })
      .catch(() => {
        status.value = "error";
      })
      .finally(() => {
        activeCheck = null;
      });

    return activeCheck;
  }

  function dismissPrompt(): void {
    promptDismissed.value = true;
  }

  async function openLatestRelease(): Promise<void> {
    if (!latestRelease.value) return;
    await openReleasePage(latestRelease.value.url);
  }

  return {
    status,
    latestRelease,
    shouldShowPrompt,
    checkForUpdates,
    dismissPrompt,
    openLatestRelease,
  };
});
