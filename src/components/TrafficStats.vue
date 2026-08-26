<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { VpnConnection } from "../types/vpn";
import { formatBytes, formatDuration } from "../utils/format";
import { useNow } from "../composables/useNow";

const props = withDefaults(
  defineProps<{
    connection: VpnConnection;
    compact?: boolean;
  }>(),
  { compact: false },
);

const { t } = useI18n();
const now = useNow();

const connectedTime = computed(() => formatDuration(props.connection.connectedAt, now.value));

</script>

<template>
  <dl class="traffic-stats" :class="{ 'traffic-stats--compact': compact }">
    <div>
      <dt>{{ t("connectionCard.metrics.time") }}</dt>
      <dd>{{ connectedTime }}</dd>
    </div>
    <div>
      <dt>{{ t("connectionCard.metrics.download") }}</dt>
      <dd><span class="traffic-arrow traffic-arrow--down">↓</span>{{ formatBytes(connection.bytesReceived) }}</dd>
    </div>
    <div>
      <dt>{{ t("connectionCard.metrics.upload") }}</dt>
      <dd><span class="traffic-arrow traffic-arrow--up">↑</span>{{ formatBytes(connection.bytesSent) }}</dd>
    </div>
  </dl>
</template>
