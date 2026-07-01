<script lang="ts">
import { Util } from './utils/Util'

const utils: Util[] = [new Util()]
</script>

{#each utils as util, index (index)}
  <p>{util.getter} {util.property} {util.hello()}</p>
{/each}
