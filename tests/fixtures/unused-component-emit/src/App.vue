<script setup lang="ts">
import Orphan from "./components/Orphan.vue";
import Renamed from "./components/Renamed.vue";
import TemplateEmit from "./components/TemplateEmit.vue";
import Dynamic from "./components/Dynamic.vue";
</script>

<template>
  <Orphan @saved="noop" />
  <Renamed @change="noop" />
  <TemplateEmit @go="noop" />
  <Dynamic @a="noop" @b="noop" />
</template>
