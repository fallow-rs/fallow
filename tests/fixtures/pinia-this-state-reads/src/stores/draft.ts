import { defineStore } from 'pinia'

declare function persist(state: object): void

export const useDraftStore = defineStore('draft', {
  state: () => ({ title: '', body: '' }),
  getters: {
    length: (state) => state.body.length,
  },
  actions: {
    save(): void {
      persist(this.$state)
    },
  },
})
