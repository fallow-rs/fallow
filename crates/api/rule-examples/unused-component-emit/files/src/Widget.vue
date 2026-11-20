<script setup lang="ts">
const emit = defineEmits<{ checkout: []; close: [] }>();
</script>

<template>
  <section>
    <button @click="emit('checkout')">Check out</button>
  </section>
</template>
