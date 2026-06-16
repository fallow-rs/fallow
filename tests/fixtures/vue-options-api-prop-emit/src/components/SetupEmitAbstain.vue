<script>
// Options-API `emits` declaration combined with a `setup(props, { emit })`
// method that fires the emit through the context binding. The `this.$emit`
// usage walk cannot see a bare `emit('done')` call, so the whole component's
// emit findings must abstain and `done` must NOT be flagged as unused.
export default {
  name: "SetupEmitAbstain",
  emits: ["done"],
  setup(props, { emit }) {
    return {
      finish() {
        emit("done");
      },
    };
  },
};
</script>

<template>
  <button @click="finish">done</button>
</template>
