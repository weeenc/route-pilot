import { onBeforeUnmount, onMounted, readonly, ref, type DeepReadonly, type Ref } from "vue";

const now = ref(Date.now());
let subscribers = 0;
let clock: ReturnType<typeof setInterval> | undefined;

function startClock(): void {
  now.value = Date.now();
  clock ??= setInterval(() => {
    now.value = Date.now();
  }, 1000);
}

function stopClock(): void {
  if (clock !== undefined) clearInterval(clock);
  clock = undefined;
}

/** Shares one wall clock across every visible traffic widget. */
export function useNow(): DeepReadonly<Ref<number>> {
  onMounted(() => {
    subscribers += 1;
    if (subscribers === 1) startClock();
  });

  onBeforeUnmount(() => {
    subscribers = Math.max(0, subscribers - 1);
    if (subscribers === 0) stopClock();
  });

  return readonly(now);
}
