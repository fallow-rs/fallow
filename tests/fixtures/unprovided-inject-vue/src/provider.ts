import { provide } from 'vue'
import { SHARED_KEY } from './keys'
import { BARREL_KEY } from './barrelKeys/def'
export function setup() {
  provide(SHARED_KEY, 1)
  provide(BARREL_KEY, 2)
}
