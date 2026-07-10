import { RESTApi } from './api'

let api: RESTApi | undefined

export function useApi(): RESTApi {
  if (!api) {
    api = new RESTApi()
  }
  return api
}

/// Not a factory: returns no class instance. `helper().anything` must resolve to no
/// proven factory export and credit nothing.
export function helper(): { anything: number } {
  return { anything: 1 }
}
