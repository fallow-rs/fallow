<script lang="ts">
  import { createEventDispatcher } from "svelte";
  const dispatch = createEventDispatcher();
</script>
<button on:click={() => dispatch("save")}>Save cart</button>
<button on:click={() => dispatch("dead")}>Empty cart</button>
