<script setup lang="ts">
import { storeToRefs } from "pinia";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";

import { isDesktopRuntime } from "../api/settings";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import ConnectionCard from "../components/ConnectionCard.vue";
import CurrentConnectionCard from "../components/CurrentConnectionCard.vue";
import ProfileEditorDialog from "../components/ProfileEditorDialog.vue";
import { translateRuntimeError } from "../i18n";
import { useProfilesStore } from "../stores/profiles";
import { useVpnStore } from "../stores/vpn";
import type { UpdateVpnProfileInput, VpnProfile } from "../types/profile";
import { canDisconnectState } from "../utils/connectionState";

interface ToastMessage {
  id: number;
  text: string;
  tone: "success" | "error";
}

const profilesStore = useProfilesStore();
const vpnStore = useVpnStore();
const { t } = useI18n();
const {
  profiles,
  isLoading,
  isImporting,
  updatingProfileIds,
  deletingProfileIds,
  errorMessage: profileError,
} = storeToRefs(profilesStore);
const {
  pendingActions,
  profileErrors,
  routeConflicts,
  errorMessage: connectionError,
} = storeToRefs(vpnStore);

const selectedProfileId = ref<string | null>(null);
const searchQuery = ref("");
const editingProfile = ref<VpnProfile | null>(null);
const deletingProfile = ref<VpnProfile | null>(null);
const toasts = ref<ToastMessage[]>([]);
const toastTimers = new Map<number, ReturnType<typeof setTimeout>>();
let nextToastId = 0;

const selectedProfile = computed(
  () => profiles.value.find((profile) => profile.id === selectedProfileId.value) ?? null,
);
const selectedConnection = computed(() =>
  selectedProfile.value ? vpnStore.connectionFor(selectedProfile.value.id) : null,
);
const selectedError = computed(() => {
  if (!selectedProfile.value) return undefined;
  return (
    profileErrors.value[selectedProfile.value.id] ??
    translateRuntimeError(selectedConnection.value?.errorMessage ?? null) ??
    undefined
  );
});
const showSearch = computed(() => profiles.value.length > 8);
const visibleProfiles = computed(() => {
  const query = searchQuery.value.trim().toLocaleLowerCase();
  if (!query) return profiles.value;
  return profiles.value.filter((profile) =>
    [profile.name, profile.serverHost, profile.protocol]
      .filter(Boolean)
      .some((value) => value!.toLocaleLowerCase().includes(query)),
  );
});
const profileNames = computed(() =>
  Object.fromEntries(profiles.value.map((profile) => [profile.id, profile.name])),
);
const preferredProfileSignature = computed(() =>
  profiles.value
    .map((profile) => `${profile.id}:${vpnStore.connectionFor(profile.id).state}`)
    .join("|"),
);

function choosePreferredProfile(): void {
  if (selectedProfile.value) return;
  const activeProfile = profiles.value.find((profile) =>
    canDisconnectState(vpnStore.connectionFor(profile.id).state),
  );
  selectedProfileId.value = activeProfile?.id ?? profiles.value[0]?.id ?? null;
}

function showToast(text: string, tone: ToastMessage["tone"]): void {
  const id = ++nextToastId;
  toasts.value.push({ id, text, tone });
  const timer = setTimeout(() => dismissToast(id), 3600);
  toastTimers.set(id, timer);
}

function dismissToast(id: number): void {
  toasts.value = toasts.value.filter((toast) => toast.id !== id);
  const timer = toastTimers.get(id);
  if (timer) clearTimeout(timer);
  toastTimers.delete(id);
}

async function importProfile(): Promise<void> {
  const profile = await profilesStore.chooseAndImport();
  if (profile) {
    vpnStore.registerProfile(profile.id);
    selectedProfileId.value = profile.id;
    showToast(t("connections.toasts.imported"), "success");
  } else if (profileError.value) {
    showToast(profileError.value, "error");
  }
}

async function connect(profile: VpnProfile): Promise<void> {
  selectedProfileId.value = profile.id;
  await vpnStore.connect(profile.id);
  if (profileErrors.value[profile.id]) {
    showToast(t("connections.toasts.connectionFailed"), "error");
  }
}

async function disconnect(profile: VpnProfile): Promise<void> {
  selectedProfileId.value = profile.id;
  await vpnStore.disconnect(profile.id);
}

async function saveProfile(input: UpdateVpnProfileInput): Promise<void> {
  if (!editingProfile.value) return;
  const didSave = await profilesStore.update(editingProfile.value.id, input);
  if (didSave) {
    editingProfile.value = null;
    showToast(t("connections.toasts.saved"), "success");
  }
}

async function removeProfile(): Promise<void> {
  if (!deletingProfile.value) return;
  const profileId = deletingProfile.value.id;
  const didDelete = await profilesStore.remove(profileId);
  if (didDelete) {
    vpnStore.forgetProfile(profileId);
    deletingProfile.value = null;
    choosePreferredProfile();
    showToast(t("connections.toasts.deleted"), "success");
  }
}

async function copyServer(profile: VpnProfile): Promise<void> {
  if (!profile.serverHost) return;
  const address = profile.serverPort
    ? `${profile.serverHost}:${profile.serverPort}`
    : profile.serverHost;
  try {
    await navigator.clipboard.writeText(address);
    showToast(t("connections.toasts.copied"), "success");
  } catch {
    showToast(t("connections.toasts.copyFailed"), "error");
  }
}

watch(preferredProfileSignature, choosePreferredProfile);

onMounted(async () => {
  await profilesStore.load();
  await vpnStore.initialize(profiles.value.map((profile) => profile.id));
  choosePreferredProfile();
});

onBeforeUnmount(() => {
  vpnStore.stopListening();
  toastTimers.forEach((timer) => clearTimeout(timer));
});
</script>

<template>
  <section class="page">
    <header class="page-header connections-header">
      <div>
        <h1>{{ t("connections.title") }}</h1>
        <p class="page-description">{{ t("connections.description") }}</p>
      </div>
      <button
        class="button button--secondary import-button"
        type="button"
        :disabled="!isDesktopRuntime || isImporting"
        @click="importProfile"
      >
        <svg aria-hidden="true" viewBox="0 0 20 20">
          <path d="M10 4v12M4 10h12" />
        </svg>
        {{ isImporting ? t("connections.importing") : t("connections.importProfile") }}
      </button>
    </header>

    <div v-if="connectionError" class="page-alert" role="alert">
      {{ connectionError }}
    </div>

    <aside v-if="routeConflicts.length" class="route-conflicts" aria-labelledby="route-conflicts-title">
      <div class="route-conflicts__icon" aria-hidden="true">!</div>
      <div>
        <h2 id="route-conflicts-title">{{ t("connections.routeConflicts.title") }}</h2>
        <p>{{ t("connections.routeConflicts.description") }}</p>
        <ul>
          <li
            v-for="conflict in routeConflicts"
            :key="`${conflict.leftProfileId}-${conflict.leftNetwork}-${conflict.rightProfileId}-${conflict.rightNetwork}`"
          >
            <strong>{{ profileNames[conflict.leftProfileId] ?? conflict.leftProfileId }}</strong>
            <code>{{ conflict.leftNetwork }}</code>
            <span>{{ t("connections.routeConflicts.overlaps") }}</span>
            <strong>{{ profileNames[conflict.rightProfileId] ?? conflict.rightProfileId }}</strong>
            <code>{{ conflict.rightNetwork }}</code>
          </li>
        </ul>
      </div>
    </aside>

    <section class="content-section" aria-labelledby="current-connection-title">
      <div class="section-heading">
        <h2 id="current-connection-title">{{ t("currentConnection.sectionTitle") }}</h2>
      </div>
      <CurrentConnectionCard
        :profile="selectedProfile"
        :connection="selectedConnection"
        :action="selectedProfile ? pendingActions[selectedProfile.id] : undefined"
        :error="selectedError"
        :desktop-available="isDesktopRuntime"
        @connect="selectedProfile && connect(selectedProfile)"
        @disconnect="selectedProfile && disconnect(selectedProfile)"
      />
    </section>

    <section class="content-section content-section--profiles" aria-labelledby="profile-list-title">
      <div class="section-heading section-heading--profiles">
        <h2 id="profile-list-title">
          {{ t("connections.profilesTitle") }}
          <span>· {{ profiles.length }}</span>
        </h2>
        <label v-if="showSearch" class="profile-search">
          <svg aria-hidden="true" viewBox="0 0 20 20">
            <circle cx="8.5" cy="8.5" r="5" />
            <path d="m12.2 12.2 4 4" />
          </svg>
          <span class="sr-only">{{ t("connections.searchLabel") }}</span>
          <input v-model="searchQuery" type="search" :placeholder="t('connections.searchPlaceholder')" />
        </label>
      </div>

      <div v-if="isLoading" class="connections-loading" aria-live="polite">
        <span class="spinner spinner--dark" aria-hidden="true"></span>
        <p>{{ t("connections.loading") }}</p>
      </div>

      <div v-else-if="visibleProfiles.length" class="profile-list" role="list">
        <ConnectionCard
          v-for="profile in visibleProfiles"
          :key="profile.id"
          :profile="profile"
          :connection="vpnStore.connectionFor(profile.id)"
          :action="pendingActions[profile.id]"
          :error="profileErrors[profile.id]"
          :selected="selectedProfileId === profile.id"
          :is-updating="Boolean(updatingProfileIds[profile.id])"
          :is-deleting="Boolean(deletingProfileIds[profile.id])"
          @select="selectedProfileId = profile.id"
          @connect="connect(profile)"
          @disconnect="disconnect(profile)"
          @edit="editingProfile = profile"
          @copy="copyServer(profile)"
          @delete="deletingProfile = profile"
        />
      </div>

      <div v-else-if="profiles.length" class="empty-state empty-state--search">
        <h3>{{ t("connections.noSearchResults") }}</h3>
        <p>{{ t("connections.noSearchResultsDescription") }}</p>
      </div>

      <div v-else class="empty-state empty-state--profiles">
        <div class="empty-state__icon" aria-hidden="true">
          <svg viewBox="0 0 24 24">
            <path d="M5 4.5h9l5 5v10H5v-15Z" />
            <path d="M14 4.5v5h5M8.5 14h7M12 10.5v7" />
          </svg>
        </div>
        <h3>{{ t("connections.empty.title") }}</h3>
        <p v-if="isDesktopRuntime">{{ t("connections.empty.desktopDescription") }}</p>
        <p v-else>{{ t("connections.empty.browserDescription") }}</p>
        <button
          class="button button--secondary"
          type="button"
          :disabled="!isDesktopRuntime || isImporting"
          @click="importProfile"
        >
          {{ t("connections.importProfile") }}
        </button>
      </div>
    </section>

    <ProfileEditorDialog
      v-if="editingProfile"
      :profile="editingProfile"
      :is-saving="Boolean(updatingProfileIds[editingProfile.id])"
      @cancel="editingProfile = null"
      @save="saveProfile"
    />

    <ConfirmDialog
      v-if="deletingProfile"
      :title="t('connections.deleteDialog.title')"
      :description="t('connections.deleteDialog.description', { name: deletingProfile.name })"
      :confirm-label="t('connections.deleteDialog.confirm')"
      :busy-label="t('connections.deleteDialog.deleting')"
      :busy="Boolean(deletingProfileIds[deletingProfile.id])"
      @cancel="deletingProfile = null"
      @confirm="removeProfile"
    />

    <div class="toast-region" aria-live="polite" aria-atomic="true">
      <button
        v-for="toast in toasts"
        :key="toast.id"
        class="toast"
        :class="`toast--${toast.tone}`"
        type="button"
        @click="dismissToast(toast.id)"
      >
        <span aria-hidden="true">{{ toast.tone === "success" ? "✓" : "×" }}</span>
        {{ toast.text }}
      </button>
    </div>
  </section>
</template>
