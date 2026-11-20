<script setup lang="ts">
import { inject } from "vue";
import { ThemeKey } from "./keys";

const theme = inject(ThemeKey);
</script>

<template>
  <main :class="theme">Your cart</main>
</template>
