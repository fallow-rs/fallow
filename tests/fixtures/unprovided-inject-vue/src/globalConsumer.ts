import { inject } from 'vue'
import { GLOBAL_KEY } from './globalKeys'
export function setup() {
  return inject(GLOBAL_KEY)
}
