import { inject } from 'vue'
export function setup() {
  return inject('stringKey')
}
