import { useCounterStore } from './stores/counter'

type CounterStore = ReturnType<typeof useCounterStore>

// Issue #1489 case 2: the store reaches the access through a typed param, never
// bound from a `useStore()` call here. Both forms are genuine uses.

// Member access on an object-wrapped typed store param.
export function memberViaTypedParam(props: { store: CounterStore }) {
  return props.store.viaTypedParam()
}

// Destructure of an object-wrapped typed store param.
export function destructureFromTypedParam(props: { store: CounterStore }) {
  const { viaParamDestructure } = props.store
  return viaParamDestructure.value
}
