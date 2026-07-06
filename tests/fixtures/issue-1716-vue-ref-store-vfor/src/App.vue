<script setup lang="ts">
import { ref } from 'vue'
import { Util } from './utils/Util'

const items = ref<Util[]>([new Util()])
const store: { entries: Util[] } = { entries: [new Util()] }
</script>

<template>
  <template v-for="(util, index) of items.value" :key="index">
    {{ util.getter }} {{ util.hello() }}
  </template>
  <template v-for="entry of store.entries" :key="entry.property">
    {{ entry.property }}
  </template>
</template>
