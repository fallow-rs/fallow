import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

export const useCounterStore = defineStore('counter', () => {
  const count = ref(1)
  // Read only through the imported alias in read.ts.
  const doubled = computed(() => count.value * 2)
  // Read only through the later-declared alias in later.ts.
  const label = computed(() => `count ${count.value}`)
  // Read only through the alias that scoped.ts declares in a function body.
  const scopedTotal = computed(() => count.value + 1)
  // Returned, but nothing reads it: must stay flagged (control).
  const deadFlag = ref(false)
  return { count, doubled, label, scopedTotal, deadFlag }
})

export type CounterStore = ReturnType<typeof useCounterStore>
