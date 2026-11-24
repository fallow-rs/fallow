import { useCounterStore } from './stores/counter'
import { useDraftStore } from './stores/draft'

const counter = useCounterStore()
export const total = counter.total()
counter.reset()

useDraftStore().save()
