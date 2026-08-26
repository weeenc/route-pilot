<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { ConnectionState } from "../types/vpn";

const props = defineProps<{
  state: ConnectionState;
}>();

const { t } = useI18n();

const labelKeys: Record<ConnectionState, string> = {
  disconnected: "connectionCard.states.disconnected",
  connecting: "connectionCard.states.connecting",
  connected: "connectionCard.states.connected",
  reconnecting: "connectionCard.states.reconnecting",
  disconnecting: "connectionCard.states.disconnecting",
  error: "connectionCard.states.error",
};

const tone = computed(() => {
  if (["connecting", "reconnecting", "disconnecting"].includes(props.state)) {
    return "connecting";
  }
  return props.state;
});
</script>

<template>
  <span
    class="status-badge"
    :class="[`status-badge--${tone}`, { 'status-badge--animated': tone === 'connecting' }]"
    aria-live="polite"
  >
    <span class="status-badge__dot" aria-hidden="true"></span>
    {{ t(labelKeys[state]) }}
  </span>
</template>
