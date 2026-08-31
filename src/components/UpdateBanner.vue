<script setup lang="ts">
import { storeToRefs } from "pinia";
import { useI18n } from "vue-i18n";

import { useUpdateStore } from "../stores/update";
import { appVersion } from "../version";

const { t } = useI18n();
const updateStore = useUpdateStore();
const { latestRelease, shouldShowPrompt } = storeToRefs(updateStore);
</script>

<template>
  <aside v-if="shouldShowPrompt && latestRelease" class="update-banner" role="alert">
    <span class="update-banner__icon" aria-hidden="true">
      <svg viewBox="0 0 24 24">
        <path d="M12 4v11m0 0 4-4m-4 4-4-4M5 19h14" />
      </svg>
    </span>
    <span class="update-banner__content">
      <strong>{{ t("updates.availableTitle", { version: latestRelease.version }) }}</strong>
      <small>{{ t("updates.availableDescription", { current: appVersion }) }}</small>
    </span>
    <button
      class="button button--primary button--small"
      type="button"
      @click="updateStore.openLatestRelease"
    >
      {{ t("updates.viewRelease") }}
    </button>
    <button
      class="icon-button update-banner__close"
      type="button"
      :aria-label="t('updates.dismiss')"
      @click="updateStore.dismissPrompt"
    >
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <path d="m6 6 8 8m0-8-8 8" />
      </svg>
    </button>
  </aside>
</template>
