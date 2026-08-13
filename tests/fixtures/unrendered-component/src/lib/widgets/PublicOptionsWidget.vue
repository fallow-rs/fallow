<script lang="ts">
// Options-API sibling of `PublicWidget`: exposed through the package's ./lib
// entry, rendered nowhere in this package, so the public-API abstain must cover
// the explicit-default spelling too.
export default {
  name: "PublicOptionsWidget",
};
</script>

<template>
  <div>public options</div>
</template>
