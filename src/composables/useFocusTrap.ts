import { onBeforeUnmount, onMounted, type Ref } from "vue";

const focusableSelector = [
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "a[href]",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

/** Keeps keyboard focus inside a modal and restores its opener on close. */
export function useFocusTrap(root: Ref<HTMLElement | null>): {
  trapFocus: (event: KeyboardEvent) => void;
} {
  let previouslyFocused: HTMLElement | null = null;

  onMounted(() => {
    previouslyFocused =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
  });

  onBeforeUnmount(() => {
    previouslyFocused?.focus({ preventScroll: true });
  });

  function trapFocus(event: KeyboardEvent): void {
    const elements = Array.from(
      root.value?.querySelectorAll<HTMLElement>(focusableSelector) ?? [],
    ).filter((element) => element.getClientRects().length > 0);
    if (!elements.length) {
      event.preventDefault();
      return;
    }

    const first = elements[0];
    const last = elements[elements.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return { trapFocus };
}
