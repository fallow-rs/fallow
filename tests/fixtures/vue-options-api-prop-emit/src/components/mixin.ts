// A trivial mixin object. Its presence in a component's `mixins:` array is the
// abstain signal; its body is irrelevant to the test.
export const sharedMixin = {
  mounted() {
    return 1;
  },
};
