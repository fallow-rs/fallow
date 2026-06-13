import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useUserStore = defineStore('user', () => {
  const name = ref('')
  const deadRef = ref(0)
  function login() { name.value = 'x' }
  function deadFn() {}
  return { name, deadRef, login, deadFn }
})
