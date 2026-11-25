import { defineStore } from 'pinia'

export const useCounterStore = defineStore('counter', {
  state: () => ({ viaProperty: 1, viaDestructure: 2, viaNested: 3, deadState: 4 }),
})
