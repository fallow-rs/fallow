<script setup lang="ts">
import { useVModel } from "@vueuse/core";

const props = defineProps<{ modelValue?: string; title?: string }>();
const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  (e: "update:title", value: string): void;
  (e: "close"): void;
}>();

// `useVModel(props)` binds `modelValue`. The template writes both refs.
const value = useVModel(props);
const title = useVModel(props, "title");
</script>

<template>
  <input v-model="title" />
  <button @click="value = ''; emit('close')">clear</button>
</template>
