import { defineStore } from 'pinia'

export const useCounterStore = defineStore('counter', {
  state: () => ({ dot: 1, bracket: 2, pattern: 3, written: 4, deadState: 5 }),
  actions: {
    total(): number {
      const { pattern } = this.$state
      return this.$state.dot + this.$state['bracket'] + pattern
    },
    reset(): void {
      // A whole `$state` write replaces state and reads no key.
      this.$state = { dot: 0, bracket: 0, pattern: 0, written: 0, deadState: 0 }
    },
  },
})
