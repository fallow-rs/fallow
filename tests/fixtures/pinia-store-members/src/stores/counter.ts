import { defineStore } from 'pinia'

export const useCounterStore = defineStore('counter', {
  state: () => ({ count: 0, deadState: 99 }),
  getters: {
    double: (s) => s.count * 2,
    deadGetter: (s) => s.count * 3,
  },
  actions: {
    increment() { this.count++ },
    deadAction() { return 0 },
  },
})
