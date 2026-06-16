<script>
// Object-form props. `shown` is read only in the template (`{{ shown }}`), so it
// must NOT be flagged (template-credit parity with the setup path).
export default {
  name: "PropUsedInTemplate",
  props: {
    shown: String,
  },
};
</script>

<template>
  <div>{{ shown }}</div>
</template>
