<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { VpnProfile } from "../types/profile";
import type { ConnectionAction, ConnectionState, VpnConnection } from "../types/vpn";
import {
  canDisconnectState,
  isConnectedState,
  isConnectingState,
} from "../utils/connectionState";
import StatusBadge from "./StatusBadge.vue";
import TrafficStats from "./TrafficStats.vue";

const props = defineProps<{
  profile: VpnProfile | null;
  connection: VpnConnection | null;
  action?: ConnectionAction;
  error?: string;
  desktopAvailable: boolean;
}>();

const emit = defineEmits<{
  connect: [];
  disconnect: [];
}>();

const { t } = useI18n();

const displayState = computed<ConnectionState>(() => {
  if (props.error) return "error";
  if (props.action === "connect") return "connecting";
  if (props.action === "disconnect") return "disconnecting";
  return props.connection?.state ?? "disconnected";
});

const endpoint = computed(() => {
  if (!props.profile?.serverHost) return t("connectionCard.serverNotSpecified");
  const server = props.profile.serverPort
    ? `${props.profile.serverHost}:${props.profile.serverPort}`
    : props.profile.serverHost;
  return `${server} · ${props.profile.protocol?.toUpperCase() ?? "OPENVPN"}`;
});

const isConnected = computed(() => isConnectedState(displayState.value));
const isConnecting = computed(() => isConnectingState(displayState.value));
const isDisconnecting = computed(() => displayState.value === "disconnecting");
const isBusy = computed(() => isConnecting.value || isDisconnecting.value);
const canDisconnect = computed(() => canDisconnectState(displayState.value));

const title = computed(() => {
  if (displayState.value === "error") return t("currentConnection.errorTitle");
  if (displayState.value === "disconnecting") {
    return t("currentConnection.disconnectingTitle", { name: props.profile?.name ?? "" });
  }
  if (isConnecting.value) {
    return t("currentConnection.connectingTitle", { name: props.profile?.name ?? "" });
  }
  if (isConnected.value) {
    return t("currentConnection.connectedTitle", { name: props.profile?.name ?? "" });
  }
  return t("currentConnection.disconnectedTitle");
});

const description = computed(() => {
  if (displayState.value === "error") {
    return props.error || t("currentConnection.errorDescription");
  }
  if (displayState.value === "disconnecting") return t("currentConnection.disconnectingDescription");
  if (isConnecting.value) return t("currentConnection.connectingDescription");
  if (isConnected.value) return t("currentConnection.protected");
  if (props.profile) return t("currentConnection.unprotected");
  return t("currentConnection.noSelection");
});

const actionLabel = computed(() => {
  if (displayState.value === "error") return t("currentConnection.retry");
  if (displayState.value === "disconnecting") return t("connectionCard.actions.disconnecting");
  if (isConnecting.value) return t("connectionCard.actions.starting");
  if (isConnected.value) return t("connectionCard.actions.disconnect");
  return t("connectionCard.actions.connect");
});

function runAction(): void {
  if (canDisconnect.value) emit("disconnect");
  else emit("connect");
}
</script>

<template>
  <article class="current-connection-card" :class="`current-connection-card--${displayState}`">
    <div class="current-connection-card__main">
      <div class="current-connection-card__icon" aria-hidden="true">
        <svg viewBox="0 0 24 24">
          <path d="M12 3.2 19 6v5.3c0 4.4-2.8 7.7-7 9.5-4.2-1.8-7-5.1-7-9.5V6l7-2.8Z" />
          <path v-if="isConnected" d="m8.7 12 2.1 2.1 4.6-4.7" />
          <path v-else-if="displayState === 'error'" d="m9.2 9.2 5.6 5.6m0-5.6-5.6 5.6" />
          <path v-else d="M9 12h6" />
        </svg>
      </div>

      <div class="current-connection-card__content">
        <StatusBadge :state="displayState" />
        <h3>{{ title }}</h3>
        <p>{{ description }}</p>
        <div v-if="profile" class="current-connection-card__endpoint">
          <span v-if="!isConnected && !isBusy && displayState !== 'error'">
            {{ t("currentConnection.selected", { name: profile.name }) }}
          </span>
          <span>{{ endpoint }}</span>
        </div>
      </div>

      <button
        class="button"
        :class="isConnected || isConnecting || isDisconnecting ? 'button--disconnect' : 'button--primary'"
        type="button"
        :disabled="!profile || !desktopAvailable || isBusy"
        @click="runAction"
      >
        <span v-if="isBusy" class="spinner" aria-hidden="true"></span>
        {{ actionLabel }}
      </button>
    </div>

    <div v-if="isConnected && connection" class="current-connection-card__details">
      <TrafficStats :connection="connection" />
      <dl v-if="displayState === 'connected'" class="connection-addresses">
        <div>
          <dt>{{ t("currentConnection.vpnAddress") }}</dt>
          <dd>{{ connection.tunnelAddress ?? "—" }}</dd>
        </div>
        <div>
          <dt>{{ t("currentConnection.serverAddress") }}</dt>
          <dd>{{ connection.remoteAddress ?? "—" }}</dd>
        </div>
      </dl>
    </div>
  </article>
</template>
