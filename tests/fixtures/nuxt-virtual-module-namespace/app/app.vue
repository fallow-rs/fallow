<script setup lang="ts">
import * as C from '#components';
import { Named } from '#components';

import { ForwardedCard, useForwarded } from './lib/forward';
import * as Forward from './lib/forward';

const card = C.Card;
const panel = Forward.ForwardedPanel;
const label = useForwarded();
</script>

<template>
  <component :is="card" />
  <component :is="panel" />
  <Named />
  <ForwardedCard :label="label" />
</template>
