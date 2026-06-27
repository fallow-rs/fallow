import { useApi } from './composable'

export function useFactory() {
  const api = useApi()
  return api.ViaFactory.call() // genuine usage of Api.ViaFactory via the factory
}
