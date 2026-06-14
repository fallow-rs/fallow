import { inject } from 'vue'
import { LIB_KEY } from 'vue-lib-keys'
export function setup() {
  return inject(LIB_KEY)
}
