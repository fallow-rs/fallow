<script lang="ts">
  export let rows: Array<{ id: string; cells: string[]; active: boolean }> = [];
  export let mode = 'grid';
</script>

<div class="widget">
  {#each rows as row (row.id)}
    <div class="row" class:active={row.active}>
      {#if row.active}
        {#each row.cells as cell}
          {#if cell.length > 3}
            <span class="wide">{cell}</span>
          {:else if cell.length > 1}
            <span class="mid">{cell}</span>
          {:else}
            <span class="none">-</span>
          {/if}
        {/each}
      {:else if mode === 'grid'}
        <span class="grid">{row.id}</span>
      {:else}
        <span class="empty">list</span>
      {/if}
    </div>
  {/each}
</div>
