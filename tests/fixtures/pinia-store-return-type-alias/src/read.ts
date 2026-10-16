import type { CounterStore } from './store'

// The alias is declared in store.ts and imported type-only.
export const readDoubled = (store: CounterStore) => store.doubled
