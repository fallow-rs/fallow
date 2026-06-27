import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useCounterStore = defineStore('counter', () => {
  const viaTypedParam = () => 3
  const viaParamDestructure = ref(0)
  // Never accessed by any consumer: must stay flagged as unused (control).
  const deadMember = () => 99
  return { viaTypedParam, viaParamDestructure, deadMember }
})
