import type { useCounterStore } from './store'

type CounterStore = ReturnType<typeof useCounterStore>

// The helpers read the store through params typed with a type-only import.
export function readDoubled(store: ReturnType<typeof useCounterStore>) {
  return store.doubled
}

export const readLabel = (store: CounterStore) => store.label
