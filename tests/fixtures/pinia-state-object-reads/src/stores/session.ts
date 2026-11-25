import { defineStore } from 'pinia'

export const useSessionStore = defineStore('session', {
  state: () => ({ user: '', expires: 0 }),
})
