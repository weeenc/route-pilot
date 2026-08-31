<script setup lang="ts">
import { storeToRefs } from "pinia";
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";

import { normalizeAppError } from "../api/errors";
import {
  enablePrivilegedHelper,
  getPrivilegedHelperStatus,
  getSettings,
  isDesktopRuntime,
  locateOpenVpn,
  setCheckForUpdatesOnStartup,
  setOpenVpnExecutable,
} from "../api/settings";
import { setLocale, supportedLocales, type SupportedLocale } from "../i18n";
import type {
  LocatedOpenVpn,
  OpenVpnSource,
  PrivilegedHelperStatus,
} from "../types/settings";
import { useUpdateStore } from "../stores/update";
import { shouldShowPrivilegedHelperSettings } from "../utils/platformSettings";
import { appVersion } from "../version";

const { t, locale } = useI18n();
const customPath = ref("");
const locatedOpenVpn = ref<LocatedOpenVpn | null>(null);
const isLoading = ref(isDesktopRuntime);
const isSaving = ref(false);
const errorMessage = ref("");
const successMessageKey = ref("");
const helperStatus = ref<PrivilegedHelperStatus | null>(null);
const isHelperLoading = ref(isDesktopRuntime);
const isEnablingHelper = ref(false);
const helperError = ref("");
const helperSuccessKey = ref("");
const manualUpdateCheck = ref(false);
const checkForUpdatesOnStartup = ref(true);
const isSavingUpdatePreference = ref(false);
const updatePreferenceError = ref("");
const updateStore = useUpdateStore();
const { latestRelease, status: updateStatus } = storeToRefs(updateStore);

const sourceLabelKeys: Record<OpenVpnSource, string> = {
  bundled: "settings.openVpn.sources.bundled",
  custom: "settings.openVpn.sources.custom",
  path: "settings.openVpn.sources.path",
  common: "settings.openVpn.sources.common",
};
const selectedLocale = computed(() => locale.value as SupportedLocale);
const successMessage = computed(() =>
  successMessageKey.value ? t(successMessageKey.value) : "",
);
const helperSuccess = computed(() =>
  helperSuccessKey.value ? t(helperSuccessKey.value) : "",
);
const showHelperSettings = computed(
  () => shouldShowPrivilegedHelperSettings(helperStatus.value),
);
const updateStatusTitle = computed(() => {
  switch (updateStatus.value) {
    case "checking":
      return t("settings.about.updates.checking");
    case "available":
      return t("settings.about.updates.available", {
        version: latestRelease.value?.version ?? "",
      });
    case "current":
      return t("settings.about.updates.current");
    case "error":
      return manualUpdateCheck.value
        ? t("settings.about.updates.failed")
        : t("settings.about.updates.automatic");
    default:
      return t("settings.about.updates.automatic");
  }
});
const updateStatusDescription = computed(() =>
  updateStatus.value === "available"
    ? t("settings.about.updates.availableDescription")
    : t("settings.about.updates.description"),
);

async function handleUpdateAction(): Promise<void> {
  if (updateStatus.value === "available") {
    await updateStore.openLatestRelease();
    return;
  }

  manualUpdateCheck.value = true;
  await updateStore.checkForUpdates(true);
}

async function handleAutomaticUpdateChange(event: Event): Promise<void> {
  const previousValue = checkForUpdatesOnStartup.value;
  const enabled = (event.target as HTMLInputElement).checked;
  checkForUpdatesOnStartup.value = enabled;
  isSavingUpdatePreference.value = true;
  updatePreferenceError.value = "";

  try {
    const settings = await setCheckForUpdatesOnStartup(enabled);
    checkForUpdatesOnStartup.value = settings.checkForUpdatesOnStartup;
  } catch (error: unknown) {
    checkForUpdatesOnStartup.value = previousValue;
    updatePreferenceError.value = normalizeAppError(error).message;
  } finally {
    isSavingUpdatePreference.value = false;
  }
}

function handleLocaleChange(event: Event): void {
  setLocale((event.target as HTMLSelectElement).value as SupportedLocale);
}

async function refreshLocation(): Promise<void> {
  errorMessage.value = "";
  try {
    locatedOpenVpn.value = await locateOpenVpn();
  } catch (error: unknown) {
    locatedOpenVpn.value = null;
    const appError = normalizeAppError(error);
    errorMessage.value =
      appError.code === "OPENVPN_NOT_FOUND"
        ? t("settings.openVpn.notFound")
        : appError.message;
  }
}

async function saveCustomPath(): Promise<void> {
  isSaving.value = true;
  errorMessage.value = "";
  successMessageKey.value = "";
  try {
    const path = customPath.value.trim();
    const settings = await setOpenVpnExecutable(path || null);
    customPath.value = settings.openvpnExecutable ?? "";
    successMessageKey.value = path
      ? "settings.openVpn.customSaved"
      : "settings.openVpn.automaticEnabled";
    await refreshLocation();
  } catch (error: unknown) {
    errorMessage.value = normalizeAppError(error).message;
  } finally {
    isSaving.value = false;
  }
}

async function useAutomaticDetection(): Promise<void> {
  customPath.value = "";
  await saveCustomPath();
}

async function refreshHelperStatus(): Promise<void> {
  helperError.value = "";
  try {
    helperStatus.value = await getPrivilegedHelperStatus();
  } catch (error: unknown) {
    helperStatus.value = null;
    helperError.value = normalizeAppError(error).message;
  } finally {
    isHelperLoading.value = false;
  }
}

async function enableHelper(): Promise<void> {
  isEnablingHelper.value = true;
  helperError.value = "";
  helperSuccessKey.value = "";
  try {
    helperStatus.value = await enablePrivilegedHelper();
    helperSuccessKey.value = "settings.helper.enabledSuccess";
  } catch (error: unknown) {
    helperError.value = normalizeAppError(error).message;
    await refreshHelperStatus();
  } finally {
    isEnablingHelper.value = false;
  }
}

function helperTitle(): string {
  if (isHelperLoading.value) return t("settings.helper.status.checking");
  switch (helperStatus.value?.state) {
    case "installed":
      return t("settings.helper.status.installed");
    case "outdated":
      return t("settings.helper.status.outdated");
    case "unavailable":
      return t("settings.helper.status.unavailable");
    case "unsupported":
      return t("settings.helper.status.unsupported");
    default:
      return t("settings.helper.status.notInstalled");
  }
}

function helperDescription(): string {
  switch (helperStatus.value?.state) {
    case "installed":
      return t("settings.helper.statusDescription.installed");
    case "outdated":
      return t("settings.helper.statusDescription.outdated");
    case "unavailable":
      return t("settings.helper.statusDescription.unavailable");
    case "unsupported":
      return t("settings.helper.statusDescription.unsupported");
    default:
      return t("settings.helper.statusDescription.notInstalled");
  }
}

onMounted(async () => {
  if (!isDesktopRuntime) {
    isLoading.value = false;
    errorMessage.value = t("settings.openVpn.desktopOnly");
    return;
  }

  try {
    const [settings] = await Promise.all([getSettings(), refreshHelperStatus()]);
    customPath.value = settings.openvpnExecutable ?? "";
    checkForUpdatesOnStartup.value = settings.checkForUpdatesOnStartup;
    await refreshLocation();
  } catch (error: unknown) {
    errorMessage.value = normalizeAppError(error).message;
  } finally {
    isLoading.value = false;
  }
});
</script>

<template>
  <section class="page page--settings">
    <header class="page-header">
      <div>
        <h1>{{ t("settings.title") }}</h1>
        <p class="page-description">{{ t("settings.description") }}</p>
      </div>
    </header>

    <div class="settings-layout">
      <section class="settings-section" aria-labelledby="interface-settings-title">
        <header>
          <h2 id="interface-settings-title">{{ t("settings.language.sectionTitle") }}</h2>
          <p>{{ t("settings.language.description") }}</p>
        </header>
        <div class="settings-panel">
          <label class="settings-row">
            <span class="settings-row__label">
              <strong>{{ t("settings.language.selectLabel") }}</strong>
              <small>{{ t("settings.language.rowDescription") }}</small>
            </span>
            <span class="select-control">
              <select
                :value="selectedLocale"
                :aria-label="t('settings.language.groupAria')"
                @change="handleLocaleChange"
              >
                <option v-for="option in supportedLocales" :key="option.code" :value="option.code">
                  {{ option.nativeName }}
                </option>
              </select>
              <svg aria-hidden="true" viewBox="0 0 20 20"><path d="m5.5 7.5 4.5 4.5 4.5-4.5" /></svg>
            </span>
          </label>
        </div>
      </section>

      <section
        v-if="showHelperSettings"
        class="settings-section"
        aria-labelledby="helper-settings-title"
      >
        <header>
          <h2 id="helper-settings-title">{{ t("settings.helper.title") }}</h2>
          <p>{{ t("settings.helper.description") }}</p>
        </header>
        <div class="settings-panel">
          <div class="settings-row settings-row--status">
            <span
              class="settings-status-dot"
              :class="{
                'settings-status-dot--ready': helperStatus?.state === 'installed',
                'settings-status-dot--error': helperStatus?.state === 'unavailable' || helperStatus?.state === 'outdated',
              }"
              aria-hidden="true"
            ></span>
            <span class="settings-row__label">
              <strong>{{ helperTitle() }}</strong>
              <small>{{ helperDescription() }}</small>
            </span>
            <button
              v-if="helperStatus?.state !== 'installed' && helperStatus?.state !== 'unsupported'"
              class="button button--secondary button--small"
              type="button"
              :disabled="!isDesktopRuntime || isHelperLoading || isEnablingHelper"
              @click="enableHelper"
            >
              <span v-if="isEnablingHelper" class="spinner spinner--dark" aria-hidden="true"></span>
              {{
                isEnablingHelper
                  ? t("settings.helper.enabling")
                  : helperStatus?.state === "outdated"
                    ? t("settings.helper.update")
                    : helperStatus?.state === "unavailable"
                      ? t("settings.helper.repair")
                      : t("settings.helper.enableOnce")
              }}
            </button>
            <button
              v-else-if="helperStatus?.state === 'installed'"
              class="button button--secondary button--small"
              type="button"
              :disabled="isHelperLoading || isEnablingHelper"
              @click="refreshHelperStatus"
            >
              {{ t("settings.helper.check") }}
            </button>
          </div>
        </div>
        <p v-if="helperError" class="form-message form-message--error" role="alert">{{ helperError }}</p>
        <p v-if="helperSuccess" class="form-message form-message--success" role="status">{{ helperSuccess }}</p>
      </section>

      <section class="settings-section" aria-labelledby="openvpn-settings-title">
        <header>
          <h2 id="openvpn-settings-title">{{ t("settings.openVpn.title") }}</h2>
          <p>{{ t("settings.openVpn.description") }}</p>
        </header>
        <div class="settings-panel">
          <div class="settings-row settings-row--status">
            <span
              class="settings-status-dot"
              :class="{ 'settings-status-dot--ready': locatedOpenVpn, 'settings-status-dot--error': !isLoading && !locatedOpenVpn }"
              aria-hidden="true"
            ></span>
            <span class="settings-row__label settings-row__label--path">
              <strong v-if="isLoading">{{ t("settings.openVpn.checking") }}</strong>
              <template v-else-if="locatedOpenVpn">
                <strong>{{ t(sourceLabelKeys[locatedOpenVpn.source]) }}</strong>
                <code :title="locatedOpenVpn.path">{{ locatedOpenVpn.path }}</code>
              </template>
              <strong v-else>{{ t("settings.openVpn.notDetected") }}</strong>
            </span>
            <button
              class="button button--secondary button--small"
              type="button"
              :disabled="!isDesktopRuntime || isLoading || isSaving"
              @click="refreshLocation"
            >
              {{ t("settings.openVpn.checkAgain") }}
            </button>
          </div>
          <div class="settings-row settings-row--field">
            <label class="settings-row__label" for="openvpn-path">
              <strong>{{ t("settings.openVpn.customPath") }}</strong>
              <small>{{ t("settings.openVpn.customPathDescription") }}</small>
            </label>
            <input
              id="openvpn-path"
              v-model="customPath"
              class="text-input"
              type="text"
              spellcheck="false"
              autocomplete="off"
              :placeholder="t('settings.openVpn.automaticPlaceholder')"
              :disabled="!isDesktopRuntime || isLoading || isSaving"
              @keydown.enter="saveCustomPath"
            />
          </div>
          <div class="settings-panel__actions">
            <button
              class="button button--secondary"
              type="button"
              :disabled="!isDesktopRuntime || isLoading || isSaving || !customPath"
              @click="useAutomaticDetection"
            >
              {{ t("settings.openVpn.useAutomatic") }}
            </button>
            <button
              class="button button--primary"
              type="button"
              :disabled="!isDesktopRuntime || isLoading || isSaving"
              @click="saveCustomPath"
            >
              <span v-if="isSaving" class="spinner" aria-hidden="true"></span>
              {{ isSaving ? t("settings.openVpn.saving") : t("settings.openVpn.savePath") }}
            </button>
          </div>
        </div>
        <p v-if="errorMessage" class="form-message form-message--error" role="alert">{{ errorMessage }}</p>
        <p v-if="successMessage" class="form-message form-message--success" role="status">{{ successMessage }}</p>
      </section>

      <section class="settings-section" aria-labelledby="about-settings-title">
        <header>
          <h2 id="about-settings-title">{{ t("settings.about.title") }}</h2>
        </header>
        <div class="settings-panel">
          <div class="settings-row">
            <span class="settings-row__label">
              <strong>RoutePilot</strong>
              <small>{{ t("settings.about.description") }}</small>
            </span>
            <span class="version">{{ t("settings.about.version", { version: appVersion }) }}</span>
          </div>
          <label class="settings-row" for="automatic-update-check">
            <span class="settings-row__label">
              <strong>{{ t("settings.about.updates.automatic") }}</strong>
              <small>{{ t("settings.about.updates.automaticDescription") }}</small>
            </span>
            <span class="toggle-control">
              <input
                id="automatic-update-check"
                type="checkbox"
                :checked="checkForUpdatesOnStartup"
                :disabled="!isDesktopRuntime || isSavingUpdatePreference"
                @change="handleAutomaticUpdateChange"
              />
              <span aria-hidden="true"></span>
            </span>
          </label>
          <div class="settings-row settings-row--status">
            <span
              class="settings-status-dot"
              :class="{
                'settings-status-dot--ready': updateStatus === 'current',
                'settings-status-dot--warning': updateStatus === 'available',
                'settings-status-dot--error': updateStatus === 'error' && manualUpdateCheck,
              }"
              aria-hidden="true"
            ></span>
            <span class="settings-row__label">
              <strong>{{ updateStatusTitle }}</strong>
              <small>{{ updateStatusDescription }}</small>
            </span>
            <button
              class="button button--secondary button--small"
              type="button"
              :disabled="updateStatus === 'checking'"
              @click="handleUpdateAction"
            >
              {{
                updateStatus === "checking"
                  ? t("settings.about.updates.checkingButton")
                  : updateStatus === "available"
                    ? t("settings.about.updates.viewRelease")
                    : t("settings.about.updates.check")
              }}
            </button>
          </div>
        </div>
        <p v-if="updatePreferenceError" class="form-message form-message--error" role="alert">
          {{ updatePreferenceError }}
        </p>
      </section>
    </div>
  </section>
</template>
