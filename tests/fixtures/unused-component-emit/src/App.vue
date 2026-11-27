<script setup lang="ts">
import Orphan from "./components/Orphan.vue";
import Renamed from "./components/Renamed.vue";
import TemplateEmit from "./components/TemplateEmit.vue";
import Dynamic from "./components/Dynamic.vue";
import VModelScript from "./components/VModelScript.vue";
import VModelTemplate from "./components/VModelTemplate.vue";
import VModelNested from "./components/VModelNested.vue";
import VModels from "./components/VModels.vue";
</script>

<template>
  <Orphan @saved="noop" />
  <Renamed @change="noop" />
  <TemplateEmit @go="noop" />
  <Dynamic @a="noop" @b="noop" />
  <VModelScript @close="noop" />
  <VModelTemplate @close="noop" />
  <VModelNested @close="noop" />
  <VModels @done="noop" />
</template>
