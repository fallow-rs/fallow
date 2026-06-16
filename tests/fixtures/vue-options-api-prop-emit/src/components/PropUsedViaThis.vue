<script>
// Array-form props. `usedViaThis` is read via `this.usedViaThis` in a computed
// getter, so it must NOT be flagged.
export default {
  name: "PropUsedViaThis",
  props: ["usedViaThis"],
  computed: {
    label() {
      return `value: ${this.usedViaThis}`;
    },
  },
};
</script>

<template>
  <div>{{ label }}</div>
</template>
