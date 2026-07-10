import { useApi } from './useApi'

const { named, ...rest } = useApi()

console.log(named, rest)
