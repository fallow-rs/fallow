import { RESTApi } from './api'

let api: RESTApi | undefined

export function useApi(): RESTApi {
  if (!api) {
    api = new RESTApi()
  }
  return api
}
