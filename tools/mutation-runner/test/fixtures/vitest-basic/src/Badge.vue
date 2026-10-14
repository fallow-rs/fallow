<template>
  <span>{{ label(count) }}</span>
</template>

<script>
export const label = (count) => (count > 1 ? "many" : "one");
</script>

<style>
.badge {
  color: red;
}
</style>
