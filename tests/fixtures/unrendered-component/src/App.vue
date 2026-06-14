<script setup lang="ts">
import { Used } from "./components";
import { registry } from "./registry";
</script>

<template>
  <Used />
  <div>{{ registry.Lazy }}</div>
</template>
