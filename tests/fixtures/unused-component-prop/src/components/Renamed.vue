<script setup lang="ts">
// `used` is destructured to a renamed local `usedLocal` and rendered: must NOT
// be flagged (FP1 regression). `deadRenamed` is declared but never read: flagged.
const { used: usedLocal, deadRenamed } = defineProps<{ used: string; deadRenamed: string }>();
</script>

<template>
  <div>{{ usedLocal }}</div>
</template>
