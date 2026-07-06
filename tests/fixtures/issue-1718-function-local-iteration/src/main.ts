import { Util } from './utils/Util'

export function render() {
  const utils: Util[] = [new Util()]
  const names = utils.map((util) => `${util.getter} ${util.hello()}`)

  for (const util of utils) {
    names.push(util.property)
  }

  return names.join(',')
}
