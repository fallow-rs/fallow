<script setup lang="ts">
import { Used, UsedOptions } from "./components";
import { ExplicitDefault } from "./components";
import { NamedOnly } from "./components";
import { registry } from "./registry";
</script>

<template>
  <Used />
  <UsedOptions />
  <ExplicitDefault />
  <span>{{ NamedOnly }}</span>
  <div>{{ registry.Lazy }}</div>
</template>
