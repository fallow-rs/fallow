<script setup lang="ts">
const mark = useMark();
const label = formatLabel(mark);
</script>
<template>
  <Banner :label="label" />
  <ExtraPanel />
</template>
