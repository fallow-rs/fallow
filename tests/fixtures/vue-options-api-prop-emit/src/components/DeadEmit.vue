<script>
// Array-form emits. `deadEmit` is declared but `this.$emit('deadEmit')` is called
// nowhere (and no template `$emit`), so it is flagged unused-component-emit.
export default {
  name: "DeadEmit",
  emits: ["deadEmit"],
  methods: {
    noop() {
      return 1;
    },
  },
};
</script>

<template>
  <div>dead emit</div>
</template>
