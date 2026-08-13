<script lang="ts">
// Options API: an explicit `export default` rather than the implicit default a
// `<script setup>` block synthesizes. Rendered as `<UsedOptions />` through the
// same barrel chain, so it must be credited exactly like `Used`.
export default {
  name: "UsedOptions",
};
</script>

<template>
  <button>used options</button>
</template>
