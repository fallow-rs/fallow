<script setup lang="ts">
export interface ComponentProps {
  message: string;
}

export interface DeadProps {
  unused: boolean;
}

const props = defineProps<ComponentProps>();
</script>

<template><span>{{ props.message }}</span></template>
