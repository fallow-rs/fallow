<script setup lang="ts">
export type ModelValue = string | number;

const model = defineModel<ModelValue>();
</script>

<template><input v-model="model" /></template>
