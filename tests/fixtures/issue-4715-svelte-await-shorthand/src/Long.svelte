<script lang="ts">
  import { delayLong } from './c';
</script>

{#await delayLong(1)}
  <p>wait</p>
{:then}
  <p>done</p>
{/await}
