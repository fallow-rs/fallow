<script setup lang="ts">
import { Used, UsedOptions } from "./components";
import { registry } from "./registry";
</script>

<template>
  <Used />
  <UsedOptions />
  <div>{{ registry.Lazy }}</div>
</template>
