<script>
  import { setContext } from 'svelte'
  import { CTX_KEY } from './keys'
  setContext(CTX_KEY, { x: 1 })
</script>
<div>provider</div>
