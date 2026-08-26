import { ref } from "vue";
import { defineStore } from "pinia";

export const useAppStore = defineStore("app", () => {
  const isNavigationOpen = ref(false);

  function openNavigation(): void {
    isNavigationOpen.value = true;
  }

  function closeNavigation(): void {
    isNavigationOpen.value = false;
  }

  function toggleNavigation(): void {
    isNavigationOpen.value = !isNavigationOpen.value;
  }

  return {
    isNavigationOpen,
    openNavigation,
    closeNavigation,
    toggleNavigation,
  };
});
