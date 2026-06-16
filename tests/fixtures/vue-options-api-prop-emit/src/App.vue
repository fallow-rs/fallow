<script>
import DeadProp from "./components/DeadProp.vue";
import PropUsedViaThis from "./components/PropUsedViaThis.vue";
import PropUsedInTemplate from "./components/PropUsedInTemplate.vue";
import DeadEmit from "./components/DeadEmit.vue";
import EmitUsedViaThis from "./components/EmitUsedViaThis.vue";
import MixinsAbstain from "./components/MixinsAbstain.vue";
import ExtendsAbstain from "./components/ExtendsAbstain.vue";
import DefineComponentForm from "./components/DefineComponentForm.vue";
import SetupEmitAbstain from "./components/SetupEmitAbstain.vue";

export default {
  components: {
    DeadProp,
    PropUsedViaThis,
    PropUsedInTemplate,
    DeadEmit,
    EmitUsedViaThis,
    MixinsAbstain,
    ExtendsAbstain,
    DefineComponentForm,
    SetupEmitAbstain,
  },
};
</script>

<template>
  <DeadProp dead-prop="x" />
  <PropUsedViaThis used-via-this="x" />
  <PropUsedInTemplate shown="x" />
  <DeadEmit />
  <EmitUsedViaThis @saved="() => {}" />
  <MixinsAbstain hidden-prop="x" @hidden="() => {}" />
  <ExtendsAbstain base-prop="x" @based="() => {}" />
  <DefineComponentForm dead-dc-prop="x" />
  <SetupEmitAbstain @done="() => {}" />
</template>
