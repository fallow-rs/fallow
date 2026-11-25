import { defineStore } from 'pinia'

export interface SystemState {
  token: string
  flags: string[]
  extra: number
  indexed: number
  typedRead: number
  deadState: boolean
}

export const useSystemStore = defineStore('system', {
  state: (): SystemState => ({
    token: '',
    flags: [],
    extra: 0,
    indexed: 0,
    typedRead: 0,
    deadState: false,
  }),
  getters: {
    deadGetter: (state) => state.flags.length,
  },
})
