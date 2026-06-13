import { defineStore } from 'pinia'
export const useCounterStore = defineStore('counter', {
  state: () => ({ count: 0, deadShared: 0 }),
  getters: { double: (s) => s.count * 2 },
  actions: { inc() { this.count++ }, deadSharedAction() {} },
})
