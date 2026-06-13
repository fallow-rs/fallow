import { defineStore } from 'pinia'
export const useSpreadStore = defineStore('sp', { state: () => ({ a: 1, b: 2 }), actions: { x() {} } })
export const useKeysStore = defineStore('ke', { state: () => ({ c: 1, d: 2 }) })
export const useDynStore = defineStore('dy', { actions: { e() {}, f() {} } })
export const useMapStore = defineStore('ma', { state: () => ({ g: 1, h: 2 }) })
