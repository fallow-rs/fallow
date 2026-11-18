<script lang="ts">
  import { delayShort } from './c';
</script>

{#await delayShort(1) then}
  <p>done</p>
{/await}
