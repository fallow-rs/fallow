import { useCounterStore } from './stores/counter'

type Store = ReturnType<typeof useCounterStore>

export const readProperty = (deps: { store: ReturnType<typeof useCounterStore> }): number =>
  deps.store.viaProperty

export const readDestructure = ({ store }: { store: ReturnType<typeof useCounterStore> }): number =>
  store.viaDestructure

export const readNested = (deps: { inner: { store: ReturnType<typeof useCounterStore> } }): number =>
  deps.inner.store.viaNested

const store: Store = useCounterStore()
export const total = readProperty({ store }) + readDestructure({ store }) + readNested({ inner: { store } })
