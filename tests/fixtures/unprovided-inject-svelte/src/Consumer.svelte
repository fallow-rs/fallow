<script>
  import { getContext } from 'svelte'
  import { CTX_KEY, DEAD_CTX } from './keys'
  const a = getContext(CTX_KEY)
  const b = getContext(DEAD_CTX)
</script>
<div>{a}{b}</div>
