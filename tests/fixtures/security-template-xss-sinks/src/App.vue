<script setup>
const userHtml = window.location.hash;
</script>

<template>
  <article v-html="userHtml"></article>
</template>
