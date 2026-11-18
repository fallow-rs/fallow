<script lang="ts">
  import { loadOrFail } from './c';
</script>

{#await loadOrFail() catch error}
  <p>{error}</p>
{/await}
