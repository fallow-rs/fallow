<script setup lang="ts">
import type { Util } from './utils/Util'

const props = defineProps<{ items: Util[] }>()

// Reference the binding so it is not reported as unused; the v-for reads it too.
void props.items.length
</script>

<template>
  <div class="app">
    <!-- Credits Util.id, Util.name, Util.getValue via the props.items v-for -->
    <div v-for="(util, i) of props.items" :key="i" class="item">
      <span>{{ util.id }}</span>
      <span>{{ util.name }}</span>
      <span>{{ util.getValue() }}</span>
    </div>
  </div>
</template>
