<script lang="ts">
  import Table from './Outer.svelte';
  export let items: Array<{ id: string; label: string; level: number }> = [];
</script>

<Table rows={[]}>
  {#snippet row(item)}
    {#if item.level > 3}
      <strong>{item.label}</strong>
    {:else if item.level > 1}
      <em>{item.label}</em>
    {:else}
      <span>{item.label}</span>
    {/if}
  {/snippet}
</Table>

<p>{items.length > 0 ? 'filled' : 'empty'}</p>
