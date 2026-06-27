import { RESTApi } from './rest-api'

// Same-file factory: returns `new RESTApi()`. Issue #1441 , a member accessed
// through the factory's return value must be credited on RESTApi.
function useApi(): RESTApi {
  return new RESTApi()
}

export function run(): number {
  const api = useApi()
  return api.Plan()
}
