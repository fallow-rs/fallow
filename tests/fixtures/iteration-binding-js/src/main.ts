import { Util } from './utils/Util'

const utils: Util[] = [new Util()]

// .map callback param typed to the element class (issue #1707 follow-up).
export const labels = utils.map((util) => util.getter)

// .forEach callback param.
utils.forEach((util) => {
  util.hello()
})

// for...of loop variable.
export function total(): number {
  let sum = 0
  for (const util of utils) {
    sum += util.property
  }
  return sum
}
