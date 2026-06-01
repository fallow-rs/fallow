<script setup lang="ts">
const user = useUserStore();
const settings = useSettingsStore();
useAdminStore();
</script>

<template>
  <main>{{ user.name }} {{ settings.theme }}</main>
</template>
