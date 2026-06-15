<script setup lang="ts">
defineProps<{ forwarded: string }>();
</script>

<template>
  <span>{{ forwarded }}</span>
</template>
