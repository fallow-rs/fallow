import { createPinia } from 'pinia'
import { readDoubled, readLabel } from './read'
import { useCounterStore } from './store'

const pinia = createPinia()
pinia.state.value = { counter: { count: 5 } }
const store = useCounterStore(pinia)
console.log(readDoubled(store), readLabel(store))
