<script setup lang="ts">
const greeting = formatSharedGreeting('Nuxt default scan fixture')

const payload: SharedGreeting = {
  message: greeting,
}
</script>

<template>
  <div>{{ payload.message }}</div>
</template>
