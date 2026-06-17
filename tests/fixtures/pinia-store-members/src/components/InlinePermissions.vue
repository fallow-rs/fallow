<script setup lang="ts">
import { storeToRefs } from 'pinia'
import { toRefs } from 'vue'
import { useUserStore } from '../stores/user'

const {
  canCreateEvents: canCreate,
  canEditFloorPlans,
} = storeToRefs(useUserStore())

const { canSeeAnalytics } = toRefs(useUserStore())
</script>

<template>
  <button v-if="canCreate" type="button">
    {{ canEditFloorPlans }} {{ canSeeAnalytics }}
  </button>
</template>
