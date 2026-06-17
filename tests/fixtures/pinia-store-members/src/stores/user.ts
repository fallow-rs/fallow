import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useUserStore = defineStore('user', () => {
  const name = ref('')
  const deadRef = ref(0)
  const canCreateEvents = ref(false)
  const canEditFloorPlans = ref(false)
  const canSeeAnalytics = ref(false)
  const deadInlinePermission = ref(true)
  function login() { name.value = 'x' }
  function deadFn() {}
  return {
    name,
    deadRef,
    canCreateEvents,
    canEditFloorPlans,
    canSeeAnalytics,
    deadInlinePermission,
    login,
    deadFn,
  }
})
