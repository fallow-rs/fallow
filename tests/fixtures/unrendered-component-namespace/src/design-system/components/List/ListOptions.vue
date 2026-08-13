<script lang="ts">
// Options API (explicit `export default`) reached through the `export * as List`
// namespace barrel and rendered as `<List.ListOptions>`.
export default {
  name: "ListOptions",
};
</script>
<template><div class="list-options"><slot /></div></template>
