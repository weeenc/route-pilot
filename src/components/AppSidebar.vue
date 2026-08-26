<script setup lang="ts">
import { RouterLink } from "vue-router";
import { useI18n } from "vue-i18n";
import { appVersion } from "../version";

defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  close: [];
}>();

const { t } = useI18n();
</script>

<template>
  <aside
    class="sidebar"
    :class="{ 'sidebar--open': open }"
    :aria-label="t('navigation.primary')"
  >
    <div class="brand">
      <img class="brand__mark" src="/routepilot-icon.png" alt="" />
      <span>RoutePilot</span>
    </div>

    <nav class="navigation">
      <RouterLink class="navigation__item" to="/" @click="emit('close')">
        <svg aria-hidden="true" viewBox="0 0 24 24">
          <path d="M12 3.5 19 6v5.3c0 4.4-2.8 7.7-7 9.5-4.2-1.8-7-5.1-7-9.5V6l7-2.5Z" />
          <path d="M8.7 12h6.6" />
        </svg>
        {{ t("navigation.connections") }}
      </RouterLink>
      <RouterLink class="navigation__item" to="/settings" @click="emit('close')">
        <svg aria-hidden="true" viewBox="0 0 24 24">
          <path d="M12 15.2a3.2 3.2 0 1 0 0-6.4 3.2 3.2 0 0 0 0 6.4Z" />
          <path d="m19 13.1 1.2 1-.1 1.2-2.2 2.2-1.3.1-1-1.2-1.7.7-.2 1.6-1.1.7H9.4l-1.1-.7-.2-1.6-1.7-.7-1 1.2-1.3-.1-2.2-2.2-.1-1.2 1.2-1V11l-1.2-1 .1-1.3 2.2-2.2 1.3-.1 1 1.2 1.7-.7.2-1.6 1.1-.7h3.2l1.1.7.2 1.6 1.7.7 1-1.2 1.3.1 2.2 2.2.1 1.3-1.2 1v2.1Z" />
        </svg>
        {{ t("navigation.settings") }}
      </RouterLink>
    </nav>

    <div class="sidebar__footer">
      {{ t("navigation.version", { version: appVersion }) }}
    </div>
  </aside>
</template>
