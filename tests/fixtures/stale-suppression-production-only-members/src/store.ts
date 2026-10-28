import { defineStore } from "pinia";
import { computed, ref } from "vue";

export const useCounterStore = defineStore("counter", () => {
  const count = ref(1);
  const debugLabel = computed(() => `count ${count.value}`);
  const doubled = computed(() => count.value * 2);
  return {
    count,
    // fallow-ignore-next-line unused-store-member -- read by devtools only
    debugLabel,
    // fallow-ignore-next-line unused-store-member -- used by index and a test
    doubled,
  };
});
