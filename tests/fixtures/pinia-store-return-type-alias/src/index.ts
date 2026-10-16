import { createPinia } from 'pinia'
import { readLabel } from './later'
import { readDoubled } from './read'
import { useCounterStore } from './store'

const store = useCounterStore(createPinia())
console.log(store.count, readDoubled(store), readLabel(store))
