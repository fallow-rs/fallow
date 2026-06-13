import { useCounterStore } from '@mono/stores'
const store = useCounterStore()
store.inc()
console.log(store.count, store.double)
