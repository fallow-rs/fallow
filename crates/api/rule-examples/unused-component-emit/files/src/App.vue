<script setup lang="ts">
import Widget from "./Widget.vue";

const checkout = (): void => console.log("checkout");
</script>

<template>
  <Widget @checkout="checkout" />
</template>
