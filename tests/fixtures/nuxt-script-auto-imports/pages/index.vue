<script setup lang="ts">
import { useExplicitThing } from '../composables/useExplicitThing';

useCounter();
formatPrice(10);
fromIndex();
sharedThing();
useCollision();
useExplicitThing();
useRoute();

const useLocalThing = () => null;
useLocalThing();

type Local = UseTypeOnly;
</script>

<template><div>index</div></template>
