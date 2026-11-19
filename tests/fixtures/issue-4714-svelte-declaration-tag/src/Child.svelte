<script lang="ts">
  import { toLower, toUpper } from './util';

  type Props = { atTag: string[]; constTag: string[] };
  let { atTag, constTag }: Props = $props();
</script>

{#if atTag.length > 0}
  {@const first = toLower(atTag[0])}
  <p>{first}</p>
{/if}
{#if true}
  {const second = $derived(toUpper(constTag[0]))}
  <p>{second}</p>
{/if}
