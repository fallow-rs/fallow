import { useFactory } from './consumerFactory'
import { useDirect } from './consumerDirect'
import { Api } from './api'

export function boot() {
  return useFactory() + useDirect(new Api())
}
