<script setup lang="ts">
import * as Icons from './vue-icons';
import Panel from './Panel.vue';
</script>

<template>
  <div>{{ Icons.VueStar }}</div>
  <Panel :icons="Icons" />
</template>
