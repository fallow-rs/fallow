<script setup lang="ts">
// `go` is emitted only in the template via `$emit`: must NOT be flagged
// (template-emit FP regression).
defineEmits<{ go: [] }>();
</script>

<template>
  <button @click="$emit('go')">template emit</button>
</template>
