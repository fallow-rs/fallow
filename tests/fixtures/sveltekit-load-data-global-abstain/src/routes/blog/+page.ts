import type { PageLoad } from './$types'

export const load: PageLoad = async () => {
  // `dead` is read by no consumer; it would normally be flagged, but a
  // project-wide reflective `Object.values(page.data)` use abstains all routes.
  return { dead: 1 }
}
