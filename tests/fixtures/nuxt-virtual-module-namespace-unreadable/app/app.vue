<script setup lang="ts">
import { registry } from './lib/registry';

const first = Object.keys(registry)[0];
</script>

<template>
  <component :is="first" />
</template>
