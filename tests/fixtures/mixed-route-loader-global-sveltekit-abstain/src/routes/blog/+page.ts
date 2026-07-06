import type { PageLoad } from './$types'

export const load: PageLoad = async () => {
  return { svelteDead: 1 }
}
