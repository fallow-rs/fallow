<script>
import { BaseComponent } from "./base";

// An Options-API `extends:` key (NOT a JS `class extends`) means a base
// component may read `baseProp` or fire `based` from another file, so the whole
// component abstains: neither is flagged even though neither is used here.
export default {
  name: "ExtendsAbstain",
  extends: BaseComponent,
  props: {
    baseProp: String,
  },
  emits: ["based"],
};
</script>

<template>
  <div>extends abstain</div>
</template>
