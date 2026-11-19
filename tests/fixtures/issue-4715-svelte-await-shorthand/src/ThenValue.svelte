<script lang="ts">
  import { loadValue } from './c';
</script>

{#await loadValue() then value}
  <p>{value}</p>
{/await}
