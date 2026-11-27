<script setup lang="ts">
import { useVModel } from "@vueuse/core";

const props = defineProps<{ modelValue: { value: string } }>();
const emit = defineEmits<{
  (e: "update:modelValue", value: { value: string }): void;
  (e: "close"): void;
}>();

// Only a nested field changes, so the ref setter never runs.
const widget = useVModel(props, "modelValue");
const pick = (id: string) => {
  widget.value.value = id;
  emit("close");
};
</script>

<template>
  <input v-model="widget.value" @change="pick('a')" />
</template>
