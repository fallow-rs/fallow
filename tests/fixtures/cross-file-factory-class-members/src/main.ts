import { helper, useApi } from './useApi'

const bound = useApi()
console.log(bound.viaSimpleBinding)

console.log(useApi().viaChainedCall)
console.log(useApi().viaChainedCallThenCall())
console.log(useApi().viaChainedThenDeep.deep)

const { viaDestructure } = useApi()
const { viaRenamedKey: renamed } = useApi()
const { viaDefaultedKey = 0 } = useApi()
const { viaNestedKey: { inner } } = useApi()

console.log(viaDestructure, renamed, viaDefaultedKey, inner)

console.log(useApi()?.viaOptionalChain)

// A non-factory callee read the same way. It resolves to no proven factory export,
// so it credits nothing and cannot suppress anything.
console.log(helper().anything)
