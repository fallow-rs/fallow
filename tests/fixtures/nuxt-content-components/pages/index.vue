<script setup lang="ts">
const { data: page } = await useAsyncData(() => queryCollection("content").first())
</script>
<template>
  <ContentRenderer v-if="page" :value="page" />
</template>
