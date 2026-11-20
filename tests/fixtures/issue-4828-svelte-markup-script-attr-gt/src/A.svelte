<script>
  import { embedUrl } from './helpers.js';

  let browser = typeof window !== 'undefined';
  let failed = $state(false);
  const handleLoad = () => {};
</script>

{#if browser}
  <script src="{embedUrl}" onerror={() => (failed = true)} onload={handleLoad}></script>
  <script src="/embed.js" onload={() => {}}></script>
  <script src="/embed.js" data-big={2 > 1}></script>
{/if}

{#if failed}
  <p>The embed did not load.</p>
{/if}
