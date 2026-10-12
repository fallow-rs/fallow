<script setup lang="ts">
export interface ComponentEmits {
  (event: 'change', value: string): void;
}

const emit = defineEmits<ComponentEmits>();
</script>

<template><button @click="emit('change', 'x')">Change</button></template>
