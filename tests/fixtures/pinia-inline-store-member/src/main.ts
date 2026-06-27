import { useCounterStore } from './stores/counter'

// Inline `useStore().member` consumption with no bound local (issue #1489 case 1):
// a property read and a method call directly on the store-factory call result.
const n = useCounterStore().count
useCounterStore().increment()
void n
