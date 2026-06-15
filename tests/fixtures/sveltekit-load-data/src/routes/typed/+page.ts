import type { PageLoad } from './$types'

export const load: PageLoad = async () => {
  // `shown` is consumed via a typed `data` prop in a component attribute;
  // `typedDead` is read by no consumer and must be flagged.
  return { shown: 1, typedDead: 2 }
}
