import { provide } from 'vue'
import { SHARED_KEY } from './keys'
import { BARREL_KEY } from './barrelKeys/def'
import { DIRECT_INJECT_KEY } from './collisionKeys'
import { DIRECT_PROVIDE_KEY } from './collisionKeys/left'
export function setup() {
  provide(SHARED_KEY, 1)
  provide(BARREL_KEY, 2)
  provide(DIRECT_PROVIDE_KEY, 3)
  provide(DIRECT_INJECT_KEY, 4)
}
