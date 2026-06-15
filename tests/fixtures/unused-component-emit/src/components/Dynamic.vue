<script setup lang="ts">
// A dynamic `emit(<nonLiteral>)` makes the emitted event unknowable, so the
// whole file abstains: neither `a` nor `b` is flagged even though `b` looks
// unemitted.
const emit = defineEmits<{ a: []; b: [] }>();
const evt = "a";
function fire() {
  emit(evt);
}
</script>

<template>
  <button @click="fire">dynamic</button>
</template>
