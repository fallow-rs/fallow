<template>
  <UPageAside />
</template>

<script setup lang="ts">
const links = [
  {
    id: 'source',
    label: 'View page source',
    icon: 'i-simple-icons-github',
    to: 'https://github.com/fallow-rs/fallow'
  },
  {
    id: 'plain-string',
    label: 'i-simple-twitter',
    to: '/plain'
  }
]
</script>
