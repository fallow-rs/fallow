<script setup lang="ts">
import { useVModels } from "@vueuse/core";

const props = defineProps<{ open: boolean; step: number }>();
const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
  (e: "update:step", value: number): void;
  (e: "done"): void;
}>();

const { open, step } = useVModels(props);
const close = () => {
  open.value = false;
  emit("done");
};
</script>

<template>
  <button v-if="step > 0" @click="close">close</button>
</template>
