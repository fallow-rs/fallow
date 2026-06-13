import { useSpreadStore, useKeysStore, useDynStore, computedOptions } from './consume'
const sp = useSpreadStore(); const copy = { ...sp }
const ke = useKeysStore(); Object.keys(ke)
const dy = useDynStore(); const k = 'e'; dy[k]()
console.log(copy, k, computedOptions)
