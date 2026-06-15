<script setup lang="ts">
import Orphan from "./components/Orphan.vue";
import Renamed from "./components/Renamed.vue";
import Forwarded from "./components/Forwarded.vue";
import Used from "./components/Used.vue";
</script>

<template>
  <Orphan dead-prop="x" />
  <Renamed used="u" dead-renamed="d" />
  <Forwarded forwarded="f" />
  <Used shown="s" />
</template>
