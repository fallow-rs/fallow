import { useUserStore } from '../stores/user'

export function setup() {
  const u = useUserStore()
  u.login()
  return { name: u.name }
}
