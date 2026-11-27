<script setup lang="ts">
import { useVModel } from "@vueuse/core";

const props = defineProps<{ modelValue?: string }>();
const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  (e: "close"): void;
}>();

// The setter of the model ref emits `update:modelValue`.
const name = useVModel(props, "modelValue");

const reset = () => {
  name.value = "";
  emit("close");
};
</script>

<template>
  <button @click="reset">{{ name }}</button>
</template>
