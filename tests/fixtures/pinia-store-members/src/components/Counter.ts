import { storeToRefs } from 'pinia'
import { useCounterStore } from '../stores/counter'

export function setup() {
  const store = useCounterStore()
  const { double } = storeToRefs(store)
  store.increment()
  return { double, count: store.count }
}
