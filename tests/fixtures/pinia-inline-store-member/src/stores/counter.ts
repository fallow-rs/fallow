import { defineStore } from 'pinia'

export const useCounterStore = defineStore('counter', {
  state: () => ({ count: 0, deadState: 99 }),
  actions: {
    increment() { this.count++ },
    deadAction() { return 0 },
  },
})
