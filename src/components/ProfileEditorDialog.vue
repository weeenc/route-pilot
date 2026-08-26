<script setup lang="ts">
import { nextTick, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";

import { useFocusTrap } from "../composables/useFocusTrap";
import type { UpdateVpnProfileInput, VpnProfile } from "../types/profile";

const props = defineProps<{
  profile: VpnProfile;
  isSaving: boolean;
}>();

const emit = defineEmits<{
  cancel: [];
  save: [input: UpdateVpnProfileInput];
}>();

const { t } = useI18n();
const draftName = ref(props.profile.name);
const draftIgnoreRedirectGateway = ref(props.profile.ignoreRedirectGateway);
const errorKey = ref("");
const nameInput = ref<HTMLInputElement | null>(null);
const dialog = ref<HTMLElement | null>(null);
const { trapFocus } = useFocusTrap(dialog);

function save(): void {
  const name = draftName.value.trim();
  if (!name) {
    errorKey.value = "connectionCard.editor.nameRequired";
    nameInput.value?.focus();
    return;
  }
  if ([...name].length > 80) {
    errorKey.value = "connectionCard.editor.nameTooLong";
    nameInput.value?.focus();
    return;
  }

  errorKey.value = "";
  emit("save", {
    name,
    ignoreRedirectGateway: draftIgnoreRedirectGateway.value,
  });
}

onMounted(async () => {
  await nextTick();
  nameInput.value?.focus();
  nameInput.value?.select();
});
</script>

<template>
  <Teleport to="body">
    <div class="dialog-backdrop" @mousedown.self="!isSaving && emit('cancel')">
      <form
        ref="dialog"
        class="dialog dialog--editor"
        role="dialog"
        aria-modal="true"
        aria-labelledby="profile-editor-title"
        @submit.prevent="save"
        @keydown.esc="!isSaving && emit('cancel')"
        @keydown.tab="trapFocus"
      >
        <div class="dialog__header">
          <div>
            <h2 id="profile-editor-title">{{ t("connectionCard.editor.title") }}</h2>
            <p>{{ t("connectionCard.editor.description") }}</p>
          </div>
          <button
            class="icon-button"
            type="button"
            :disabled="isSaving"
            :aria-label="t('common.close')"
            @click="emit('cancel')"
          >
            <svg aria-hidden="true" viewBox="0 0 20 20">
              <path d="m5 5 10 10M15 5 5 15" />
            </svg>
          </button>
        </div>

        <label class="form-field">
          <span>{{ t("connectionCard.editor.name") }}</span>
          <input
            ref="nameInput"
            v-model="draftName"
            type="text"
            maxlength="80"
            autocomplete="off"
            :disabled="isSaving"
          />
          <small>{{ t("connectionCard.editor.nameHint") }}</small>
        </label>

        <label class="checkbox-row">
          <input
            v-model="draftIgnoreRedirectGateway"
            type="checkbox"
            :disabled="isSaving"
          />
          <span>
            <strong>{{ t("connectionCard.editor.keepInternetOutside") }}</strong>
            <small>{{ t("connectionCard.editor.keepInternetOutsideHint") }}</small>
          </span>
        </label>

        <p v-if="errorKey" class="form-message form-message--error" role="alert">
          {{ t(errorKey) }}
        </p>

        <div class="dialog__actions">
          <button class="button button--secondary" type="button" :disabled="isSaving" @click="emit('cancel')">
            {{ t("common.cancel") }}
          </button>
          <button class="button button--primary" type="submit" :disabled="isSaving">
            <span v-if="isSaving" class="spinner" aria-hidden="true"></span>
            {{ isSaving ? t("connectionCard.editor.saving") : t("connectionCard.editor.saveChanges") }}
          </button>
        </div>
      </form>
    </div>
  </Teleport>
</template>
