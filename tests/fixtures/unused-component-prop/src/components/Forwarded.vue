<script setup lang="ts">
import Child from "./Child.vue";
const myProps = defineProps<{ forwarded: string }>();
</script>

<template>
  <Child v-bind="myProps" />
</template>
