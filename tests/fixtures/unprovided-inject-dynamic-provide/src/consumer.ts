import { inject } from 'vue'
import { A_KEY } from './keys'
export function setup() {
  return inject(A_KEY)
}
