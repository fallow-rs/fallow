<script setup lang="ts">
import { LazyDialogCard, UsedCard } from '#components';
import { useUsed } from '#imports';

import { cardsEnabled } from './lib/cards';

const cards = [UsedCard, LazyDialogCard];
const label = useUsed();
</script>

<template>
  <component :is="cards[0]" v-if="cardsEnabled" :label="label" />
  <component :is="cards[1]" />
</template>
