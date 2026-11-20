<script lang="ts">
  import Child from "./Child.svelte";

  function saveCart(): void {
    console.log("cart saved");
  }
</script>

<Child on:save={saveCart} />
