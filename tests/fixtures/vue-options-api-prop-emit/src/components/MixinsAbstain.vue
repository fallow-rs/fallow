<script>
import { sharedMixin } from "./mixin";

// A `mixins:` array means a mixin can read `hiddenProp` or fire `hidden`
// invisibly to the per-component scan, so the whole component abstains: neither
// the prop nor the emit is flagged even though neither is used in this file.
export default {
  name: "MixinsAbstain",
  mixins: [sharedMixin],
  props: {
    hiddenProp: String,
  },
  emits: ["hidden"],
};
</script>

<template>
  <div>mixins abstain</div>
</template>
