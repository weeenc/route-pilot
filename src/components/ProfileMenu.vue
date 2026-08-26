<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";

defineProps<{
  profileName: string;
  canCopy: boolean;
  canDelete: boolean;
}>();

const emit = defineEmits<{
  edit: [];
  copy: [];
  delete: [];
}>();

const { t } = useI18n();
const isOpen = ref(false);
const root = ref<HTMLElement | null>(null);
const trigger = ref<HTMLButtonElement | null>(null);
const popover = ref<HTMLElement | null>(null);

function close(restoreFocus = false): void {
  isOpen.value = false;
  if (restoreFocus) void nextTick(() => trigger.value?.focus());
}

function toggle(): void {
  isOpen.value ? close() : (isOpen.value = true);
}

function choose(action: "edit" | "copy" | "delete"): void {
  close(true);
  if (action === "edit") emit("edit");
  else if (action === "copy") emit("copy");
  else emit("delete");
}

function handlePointerDown(event: PointerEvent): void {
  if (!root.value?.contains(event.target as Node)) close();
}

function handleKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") close(true);
}

function addGlobalListeners(): void {
  document.addEventListener("pointerdown", handlePointerDown);
  document.addEventListener("keydown", handleKeydown);
}

function removeGlobalListeners(): void {
  document.removeEventListener("pointerdown", handlePointerDown);
  document.removeEventListener("keydown", handleKeydown);
}

function handleMenuKeydown(event: KeyboardEvent): void {
  const items = Array.from(
    popover.value?.querySelectorAll<HTMLButtonElement>('button:not([disabled])') ?? [],
  );
  if (!items.length) return;

  const currentIndex = items.indexOf(document.activeElement as HTMLButtonElement);
  let nextIndex: number | null = null;
  if (event.key === "ArrowDown") nextIndex = (currentIndex + 1) % items.length;
  else if (event.key === "ArrowUp") nextIndex = (currentIndex - 1 + items.length) % items.length;
  else if (event.key === "Home") nextIndex = 0;
  else if (event.key === "End") nextIndex = items.length - 1;
  else if (event.key === "Escape") {
    event.stopPropagation();
    close(true);
    return;
  }

  if (nextIndex !== null) {
    event.preventDefault();
    items[nextIndex].focus();
  }
}

watch(isOpen, async (open) => {
  removeGlobalListeners();
  if (!open) return;
  addGlobalListeners();
  await nextTick();
  popover.value?.querySelector<HTMLButtonElement>('button:not([disabled])')?.focus();
});

onBeforeUnmount(removeGlobalListeners);
</script>

<template>
  <div ref="root" class="profile-menu">
    <button
      ref="trigger"
      class="icon-button"
      type="button"
      :title="t('connectionCard.menu.more')"
      :aria-label="t('connectionCard.menu.moreFor', { name: profileName })"
      :aria-expanded="isOpen"
      aria-haspopup="menu"
      @click.stop="toggle"
    >
      <svg aria-hidden="true" viewBox="0 0 20 20">
        <circle cx="4" cy="10" r="1.25" />
        <circle cx="10" cy="10" r="1.25" />
        <circle cx="16" cy="10" r="1.25" />
      </svg>
    </button>

    <div
      v-if="isOpen"
      ref="popover"
      class="profile-menu__popover"
      role="menu"
      @click.stop
      @keydown="handleMenuKeydown"
    >
      <button type="button" role="menuitem" @click="choose('edit')">
        <svg aria-hidden="true" viewBox="0 0 20 20">
          <path d="m12.8 4.2 3 3M4.5 15.5l2.9-.6 8.1-8.1a1.5 1.5 0 0 0 0-2.1l-.2-.2a1.5 1.5 0 0 0-2.1 0l-8.1 8.1-.6 2.9Z" />
        </svg>
        {{ t("connectionCard.menu.edit") }}
      </button>
      <button v-if="canCopy" type="button" role="menuitem" @click="choose('copy')">
        <svg aria-hidden="true" viewBox="0 0 20 20">
          <rect x="6.5" y="6.5" width="9" height="9" rx="1.5" />
          <path d="M13.5 6.5V5A1.5 1.5 0 0 0 12 3.5H5A1.5 1.5 0 0 0 3.5 5v7A1.5 1.5 0 0 0 5 13.5h1.5" />
        </svg>
        {{ t("connectionCard.menu.copyServer") }}
      </button>
      <div class="profile-menu__separator" role="separator"></div>
      <button
        class="profile-menu__danger"
        type="button"
        role="menuitem"
        :disabled="!canDelete"
        :title="canDelete ? undefined : t('connectionCard.menu.disconnectToDelete')"
        @click="choose('delete')"
      >
        <svg aria-hidden="true" viewBox="0 0 20 20">
          <path d="M4 6h12M8 3.5h4M6 6l.6 10h6.8L14 6M8.3 9v4.5M11.7 9v4.5" />
        </svg>
        {{ t("connectionCard.menu.delete") }}
      </button>
    </div>
  </div>
</template>
