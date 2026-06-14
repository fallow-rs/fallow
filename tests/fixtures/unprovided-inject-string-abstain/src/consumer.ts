import { inject } from 'vue'

// String-literal const used as a DI key: STRING identity, not a symbol.
// A provider (often inside a package) supplies the literal, so abstain.
const JSONFORMS_KEY = 'jsonforms'

export function setup() {
  const a = inject('stringKey')
  const b = inject(JSONFORMS_KEY)
  return { a, b }
}
