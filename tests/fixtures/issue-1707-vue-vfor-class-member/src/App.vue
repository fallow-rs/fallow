<script setup lang="ts">
import { computed } from 'vue'
import { Util } from './utils/Util'

const utils = computed(() => {
  const utls: Util[] = []

  for (let index = 0; index < 10; index++) {
    utls.push(new Util())
  }

  return utls
})
</script>

<template>
  <template v-for="(util, index) of utils" :key="index">
    {{ util.getter }}

    {{ util.property }}

    {{ util.hello() }}
  </template>
</template>
