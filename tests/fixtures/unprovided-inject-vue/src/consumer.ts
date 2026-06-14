import { inject } from 'vue'
import { SHARED_KEY, THEME_KEY } from './keys'
import { BARREL_KEY } from './barrelKeys'
export function setup() {
  const a = inject(SHARED_KEY)
  const b = inject(THEME_KEY)
  const c = inject(BARREL_KEY)
  return { a, b, c }
}
