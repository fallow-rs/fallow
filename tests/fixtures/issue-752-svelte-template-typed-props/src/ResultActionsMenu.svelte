<script lang="ts">
  import type { ResultState } from "./result-state";

  interface Props {
    resultState: ResultState;
    title: string;
  }

  let { resultState, title }: Props = $props();

  function handleOpen() {
    // Script-level member call: resolves through the typed prop binding.
    resultState.onOpen();
  }
</script>

<h1>{title}</h1>
<button onclick={handleOpen}>Open</button>
<button onclick={() => resultState.pin("x")}>Pin</button>
<input bind:value={resultState.labelInput} />
{#if resultState.labelMessage}
  <span onclick={() => resultState.addSkipRule("z")}>Skip</span>
  <button onclick={() => resultState.updateLabel("y")}>Label</button>
{/if}
