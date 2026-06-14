<script setup lang="ts">
defineProps<{ label?: string }>();
</script>

<template>
  <button>{{ label ?? "used" }}</button>
</template>
