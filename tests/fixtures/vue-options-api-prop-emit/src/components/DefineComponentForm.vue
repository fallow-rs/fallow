<script>
import { defineComponent } from "vue";

// `defineComponent({ ... })` wraps the same options object. `deadDcProp` is read
// nowhere (flagged); `usedDcProp` is read via `this.usedDcProp` (not flagged).
export default defineComponent({
  name: "DefineComponentForm",
  props: {
    deadDcProp: String,
    usedDcProp: String,
  },
  computed: {
    label() {
      return this.usedDcProp;
    },
  },
});
</script>

<template>
  <div>{{ label }}</div>
</template>
