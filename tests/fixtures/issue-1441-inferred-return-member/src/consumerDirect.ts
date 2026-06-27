import { Api } from './api'

export function useDirect(api: Api) {
  return api.Direct.call() // genuine usage of Api.Direct via a directly-typed param
}
