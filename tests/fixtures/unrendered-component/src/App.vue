<script setup lang="ts">
import { Used } from "./components";
import { ExplicitDefault } from "./components";
import { registry } from "./registry";
</script>

<template>
  <Used />
  <ExplicitDefault />
  <div>{{ registry.Lazy }}</div>
</template>
