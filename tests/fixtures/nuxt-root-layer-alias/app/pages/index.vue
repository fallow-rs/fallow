<script setup lang="ts">
import { resolveBranding } from '#layers/feedlog/shared/lib/branding'

const branding = resolveBranding('acme')
</script>

<template>
  <div>{{ branding }}</div>
</template>
