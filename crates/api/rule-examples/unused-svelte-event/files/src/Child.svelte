<script lang="ts">
  import { createEventDispatcher } from "svelte";

  const dispatch = createEventDispatcher();

  function save(): void {
    dispatch("save");
  }

  function clear(): void {
    dispatch("clear");
  }
</script>

<button on:click={save}>Save cart</button>
<button on:click={clear}>Empty cart</button>
