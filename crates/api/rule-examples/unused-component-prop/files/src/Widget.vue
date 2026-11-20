<script setup lang="ts">
defineProps<{ title: string; size: string }>();
</script>

<template>
  <section>
    <h2>{{ title }}</h2>
  </section>
</template>
