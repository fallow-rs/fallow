import { inject } from 'vue'
import { KEY } from './keys'
export function setup() {
  return inject(KEY)
}
