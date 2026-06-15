<script setup lang="ts">
// Public-API component: re-exported through the package's ./lib entry via a
// multi-hop barrel chain (entry -> export * -> widgets/index.ts -> .vue), for a
// downstream consumer to render. Rendered nowhere in THIS package, but must be
// abstained, not flagged (the deep public-API-chain abstain).
</script>

<template>
  <div>public</div>
</template>
