import type { useCounterStore } from './store'

// The alias is declared after the helper that uses it.
export const readLabel = (store: LaterStore) => store.label

type LaterStore = ReturnType<typeof useCounterStore>
