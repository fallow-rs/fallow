<script>
// Object-form props declaration. `deadProp` is declared but read nowhere in the
// component (no `this.deadProp`, no template ref): flagged unused-component-prop.
export default {
  name: "DeadProp",
  props: {
    deadProp: { type: String, required: true },
  },
  methods: {
    noop() {
      return 1;
    },
  },
};
</script>

<template>
  <div>dead prop</div>
</template>
