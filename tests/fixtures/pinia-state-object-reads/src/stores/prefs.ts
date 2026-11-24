import { defineStore } from 'pinia'

export interface PrefsState {
  theme: string
  locale: string
}

export const usePrefsStore = defineStore('prefs', {
  state: (): PrefsState => ({ theme: 'light', locale: 'en' }),
})
