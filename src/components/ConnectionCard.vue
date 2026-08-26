<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { VpnProfile } from "../types/profile";
import type { ConnectionAction, ConnectionState, VpnConnection } from "../types/vpn";
import { canDisconnectState, isConnectedState } from "../utils/connectionState";
import ProfileMenu from "./ProfileMenu.vue";
import StatusBadge from "./StatusBadge.vue";
import TrafficStats from "./TrafficStats.vue";

const props = defineProps<{
  profile: VpnProfile;
  connection: VpnConnection;
  action?: ConnectionAction;
  error?: string;
  selected: boolean;
  isUpdating: boolean;
  isDeleting: boolean;
}>();

const emit = defineEmits<{
  select: [];
  connect: [];
  disconnect: [];
  edit: [];
  copy: [];
  delete: [];
}>();

const { t } = useI18n();

const displayState = computed<ConnectionState>(() => {
  if (props.error) return "error";
  if (props.action === "connect") return "connecting";
  if (props.action === "disconnect") return "disconnecting";
  return props.connection.state;
});
const canDisconnect = computed(() => canDisconnectState(displayState.value));
const isBusy = computed(() => props.action !== undefined || displayState.value === "disconnecting");
const showMetrics = computed(() => isConnectedState(displayState.value));
const server = computed(() => {
  if (!props.profile.serverHost) return t("connectionCard.serverNotSpecified");
  return props.profile.serverPort
    ? `${props.profile.serverHost}:${props.profile.serverPort}`
    : props.profile.serverHost;
});
const endpoint = computed(
  () => `${server.value} · ${props.profile.protocol?.toUpperCase() ?? "OPENVPN"}`,
);
const actionLabel = computed(() => {
  if (props.action === "connect") return t("connectionCard.actions.starting");
  if (props.action === "disconnect" || displayState.value === "disconnecting") {
    return t("connectionCard.actions.disconnecting");
  }
  return canDisconnect.value
    ? t("connectionCard.actions.disconnect")
    : t("connectionCard.actions.connect");
});

function runAction(): void {
  emit("select");
  if (canDisconnect.value) emit("disconnect");
  else emit("connect");
}

function selectFromKeyboard(): void {
  emit("select");
}
</script>

<template>
  <article
    class="profile-card"
    :class="[
      `profile-card--${displayState}`,
      { 'profile-card--selected': selected },
    ]"
    role="listitem"
    :aria-current="selected ? 'true' : undefined"
    :aria-label="t('connectionCard.selectAria', { name: profile.name })"
    tabindex="0"
    @click="emit('select')"
    @keydown.enter.prevent="selectFromKeyboard"
    @keydown.space.prevent="selectFromKeyboard"
  >
    <span v-if="selected" class="profile-card__selection" aria-hidden="true"></span>
    <div class="profile-card__icon" aria-hidden="true">
      <svg viewBox="0 0 24 24">
        <circle cx="12" cy="12" r="2.5" />
        <path d="M5 8.2a9 9 0 0 1 14 0M7.8 10.5a5.6 5.6 0 0 1 8.4 0M12 14.5v5.2" />
      </svg>
    </div>

    <div class="profile-card__identity">
      <h3>{{ profile.name }}</h3>
      <p :title="endpoint">{{ endpoint }}</p>
      <StatusBadge :state="displayState" />
    </div>

    <TrafficStats v-if="showMetrics" :connection="connection" compact />

    <div class="profile-card__actions" @click.stop>
      <button
        class="button button--small"
        :class="selected && !canDisconnect ? 'button--soft-primary' : 'button--secondary'"
        type="button"
        :disabled="isBusy || isDeleting"
        @click="runAction"
      >
        <span v-if="isBusy" class="spinner spinner--dark" aria-hidden="true"></span>
        {{ actionLabel }}
      </button>
      <ProfileMenu
        :profile-name="profile.name"
        :can-copy="Boolean(profile.serverHost)"
        :can-delete="!canDisconnect && !isDeleting"
        @edit="emit('edit')"
        @copy="emit('copy')"
        @delete="emit('delete')"
      />
    </div>
  </article>
</template>
