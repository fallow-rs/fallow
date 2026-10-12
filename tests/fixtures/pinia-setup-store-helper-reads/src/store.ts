import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

export const useCounterStore = defineStore('counter', () => {
  // Returned state that only the `doubled` getter reads. Pinia needs it in the
  // return for registration and hydration, so it is live (issue #3348).
  const count = ref(1)
  const doubled = computed(() => count.value * 2)
  // Read only through the alias-typed helper in read.ts.
  const label = computed(() => `count ${count.value}`)
  // Returned, but nothing reads it: must stay flagged (control).
  const deadFlag = ref(false)
  return { count, doubled, label, deadFlag }
})
