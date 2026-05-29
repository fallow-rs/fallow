// Meta-preset factory call: the individual plugins are never named here, they
// are pulled in transitively by @scope/eslint-config. See issue #754.
import config from '@scope/eslint-config'

export default config({}).append({
  rules: { curly: 'off' },
})
