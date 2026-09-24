<script setup lang="ts">
const cart = useCartStore();
</script>

<template>
  <Badge :count="cart.items.length" />
</template>
