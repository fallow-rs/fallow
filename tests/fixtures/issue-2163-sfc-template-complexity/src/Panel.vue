<template>
  <div class="panel">
    <div v-for="row in rows" :key="row.id" class="row">
      <template v-if="row.active">
        <span v-for="cell in row.cells" :key="cell" class="cell">
          <b v-if="cell.length > 3">{{ cell }}</b>
          <i v-else-if="cell.length > 1">{{ cell }}</i>
          <u v-else-if="cell.length === 1">{{ cell }}</u>
          <s v-else>-</s>
        </span>
      </template>
      <span v-else-if="mode === 'grid'" class="empty">grid</span>
      <span v-else class="empty">list</span>
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  rows: Array<{ id: string; cells: string[]; active: boolean }>;
  mode: string;
}>();
</script>
