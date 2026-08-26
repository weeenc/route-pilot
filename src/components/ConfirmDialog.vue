<script setup lang="ts">
import { nextTick, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useFocusTrap } from "../composables/useFocusTrap";

defineProps<{
  title: string;
  description: string;
  confirmLabel: string;
  busyLabel: string;
  busy?: boolean;
}>();

const emit = defineEmits<{
  cancel: [];
  confirm: [];
}>();

const { t } = useI18n();
const cancelButton = ref<HTMLButtonElement | null>(null);
const dialog = ref<HTMLElement | null>(null);
const { trapFocus } = useFocusTrap(dialog);

onMounted(async () => {
  await nextTick();
  cancelButton.value?.focus();
});
</script>

<template>
  <Teleport to="body">
    <div class="dialog-backdrop" @mousedown.self="!busy && emit('cancel')">
      <section
        ref="dialog"
        class="dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-dialog-title"
        aria-describedby="confirm-dialog-description"
        @keydown.esc="!busy && emit('cancel')"
        @keydown.tab="trapFocus"
      >
        <div class="dialog__icon dialog__icon--danger" aria-hidden="true">
          <svg viewBox="0 0 24 24">
            <path d="M12 3.5 21 19H3L12 3.5Z" />
            <path d="M12 9v4.5M12 16.5v.1" />
          </svg>
        </div>
        <h2 id="confirm-dialog-title">{{ title }}</h2>
        <p id="confirm-dialog-description">{{ description }}</p>
        <div class="dialog__actions">
          <button
            ref="cancelButton"
            class="button button--secondary"
            type="button"
            :disabled="busy"
            @click="emit('cancel')"
          >
            {{ t("common.cancel") }}
          </button>
          <button class="button button--danger" type="button" :disabled="busy" @click="emit('confirm')">
            <span v-if="busy" class="spinner" aria-hidden="true"></span>
            {{ busy ? busyLabel : confirmLabel }}
          </button>
        </div>
      </section>
    </div>
  </Teleport>
</template>
