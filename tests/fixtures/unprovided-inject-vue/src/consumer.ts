import { inject } from 'vue'
import { SHARED_KEY, THEME_KEY } from './keys'
import { BARREL_KEY } from './barrelKeys'
import { DIRECT_PROVIDE_KEY } from './collisionKeys'
import { DIRECT_INJECT_KEY } from './collisionKeys/left'
export function setup() {
  const a = inject(SHARED_KEY)
  const b = inject(THEME_KEY)
  const c = inject(BARREL_KEY)
  const d = inject(DIRECT_PROVIDE_KEY)
  const e = inject(DIRECT_INJECT_KEY)
  return { a, b, c, d, e }
}
