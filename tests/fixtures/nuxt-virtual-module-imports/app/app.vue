<script setup lang="ts">
import { LazyDialogCard, UsedCard } from '#components';
import { useUsed } from '#imports';

const cards = [UsedCard, LazyDialogCard];
const label = useUsed();
</script>

<template>
  <component :is="cards[0]" :label="label" />
  <component :is="cards[1]" />
</template>
