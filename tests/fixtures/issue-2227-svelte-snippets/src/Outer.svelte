<script lang="ts">
  import Body from './Body.svelte';
  export let rows: Array<{ id: string; cells: string[]; active: boolean }> = [];
  export let mode = 'grid';
</script>

<div class="widget">
  {#each rows as row (row.id)}
    <div class="row" class:active={row.active}>
      <Body row={row} mode={mode} />
    </div>
  {/each}
</div>
