<script setup lang="ts">
// The emit binding is renamed to `emitFn` and the event is emitted through it:
// must NOT be flagged (renamed-binding FP regression).
const emitFn = defineEmits<{ change: [] }>();
function onClick() {
  emitFn("change");
}
</script>

<template>
  <button @click="onClick">renamed</button>
</template>
