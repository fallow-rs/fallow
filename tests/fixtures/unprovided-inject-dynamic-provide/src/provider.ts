import { provide } from 'vue'
import { A_KEY, B_KEY } from './keys'
export function setup(keys: symbol[]) {
  const all = [A_KEY, B_KEY, ...keys]
  all.forEach((k) => provide(k, 1))
}
