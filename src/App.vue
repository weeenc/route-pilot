<script setup lang="ts">
import { storeToRefs } from "pinia";
import { onMounted } from "vue";
import { RouterView } from "vue-router";
import { useI18n } from "vue-i18n";

import AppSidebar from "./components/AppSidebar.vue";
import UpdateBanner from "./components/UpdateBanner.vue";
import { getSettings, isDesktopRuntime } from "./api/settings";
import { useAppStore } from "./stores/app";
import { useUpdateStore } from "./stores/update";

const appStore = useAppStore();
const { isNavigationOpen } = storeToRefs(appStore);
const { t } = useI18n();
const updateStore = useUpdateStore();

onMounted(async () => {
  if (!isDesktopRuntime) {
    void updateStore.checkForUpdates();
    return;
  }

  try {
    const settings = await getSettings();
    if (settings.checkForUpdatesOnStartup) {
      void updateStore.checkForUpdates();
    }
  } catch {
    // Keep the default behavior if settings cannot be read.
    void updateStore.checkForUpdates();
  }
});
</script>

<template>
  <div class="app-shell">
    <AppSidebar :open="isNavigationOpen" @close="appStore.closeNavigation" />
    <button
      v-if="isNavigationOpen"
      class="navigation-backdrop"
      type="button"
      :aria-label="t('app.closeNavigation')"
      @click="appStore.closeNavigation"
    ></button>

    <main class="main-content">
      <button
        class="menu-button"
        type="button"
        :aria-label="t('app.openNavigation')"
        @click="appStore.toggleNavigation"
      >
        <span></span>
        <span></span>
        <span></span>
      </button>
      <UpdateBanner />
      <RouterView />
    </main>
  </div>
</template>
