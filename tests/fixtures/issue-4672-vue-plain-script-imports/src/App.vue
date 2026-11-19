<script lang="ts">
import {
  importedButUnused,
  shadowedInScriptSetup,
  usedInPlainScript,
  usedInScriptSetup,
  usedInTemplate,
} from "./utils";

export const fromPlainScript = usedInPlainScript();
</script>

<script lang="ts" setup>
const value = usedInScriptSetup();
const double = (shadowedInScriptSetup: number): number => shadowedInScriptSetup * 2;
</script>

<template>
  <div>{{ value }} {{ usedInTemplate }} {{ double(1) }}</div>
</template>
