<script setup lang="ts">
import { List, Popover } from "@/design-system";

const items = [{ id: 1, name: "first" }];
</script>

<template>
  <List.ListRoot>
    <List.ListItem v-for="x in items" :key="x.id">{{ x.name }}</List.ListItem>
  </List.ListRoot>
  <Popover.PopoverRoot>
    <Popover.PopoverContent>hi</Popover.PopoverContent>
  </Popover.PopoverRoot>
</template>
