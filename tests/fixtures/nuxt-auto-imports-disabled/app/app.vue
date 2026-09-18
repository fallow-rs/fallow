<script setup lang="ts">
import UsedCard from './components/UsedCard.vue';
import { useUsed } from './composables/useUsed';

const label = useUsed();
</script>

<template>
  <UsedCard :label="label" />
</template>
