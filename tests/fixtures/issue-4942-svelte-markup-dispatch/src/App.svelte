<script lang="ts">
  import Child from "./Child.svelte";
  const saveCart = () => {};
</script>
<Child on:save={saveCart} />
