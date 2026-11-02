<script setup>
const previewHtml = window.location.hash;
</script>

<template>
  <iframe title="preview" :srcdoc="previewHtml"></iframe>
</template>
