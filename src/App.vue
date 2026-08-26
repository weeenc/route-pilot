<script setup lang="ts">
import { storeToRefs } from "pinia";
import { RouterView } from "vue-router";
import { useI18n } from "vue-i18n";

import AppSidebar from "./components/AppSidebar.vue";
import { useAppStore } from "./stores/app";

const appStore = useAppStore();
const { isNavigationOpen } = storeToRefs(appStore);
const { t } = useI18n();
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
      <RouterView />
    </main>
  </div>
</template>
