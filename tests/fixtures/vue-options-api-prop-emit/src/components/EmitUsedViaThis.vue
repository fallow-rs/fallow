<script>
// Object-form emits. `saved` is fired via `this.$emit('saved')` in a method, so
// it must NOT be flagged. `unfired` is declared and never emitted: flagged.
export default {
  name: "EmitUsedViaThis",
  emits: {
    saved: null,
    unfired: null,
  },
  methods: {
    save() {
      this.$emit("saved", { ok: true });
    },
  },
};
</script>

<template>
  <button @click="save">save</button>
</template>
