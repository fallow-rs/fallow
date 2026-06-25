import { makeDirect } from './barrel'
import { notAFactory, useAliased, useApi } from './composables'

export function run(): number {
  const a = useApi()
  const b = makeDirect()
  const c = useAliased()
  const n = notAFactory()
  return a.Plan() + b.Material() + c.Settings() + n.Ghost()
}
