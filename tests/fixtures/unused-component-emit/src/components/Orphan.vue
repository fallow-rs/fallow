<script setup lang="ts">
// `saved` is emitted in script and must NOT be flagged. `dead` is declared but
// emitted nowhere: flagged.
const emit = defineEmits<{ saved: []; dead: [] }>();
function onClick() {
  emit("saved");
}
</script>

<template>
  <button @click="onClick">orphan</button>
</template>
