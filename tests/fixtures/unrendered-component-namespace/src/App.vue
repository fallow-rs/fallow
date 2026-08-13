<script setup lang="ts">
import { Ambiguous, List, Popover, Shadowed } from "@/design-system";
import type { TypeOnly } from "@/design-system";

const items = [{ id: 1, name: "first" }];
type ComponentType = TypeOnly;
const typeMarker: ComponentType | undefined = undefined;
void typeMarker;
</script>

<template>
  <List.ListRoot>
    <List.ListItem v-for="x in items" :key="x.id">{{ x.name }}</List.ListItem>
    <List.ListOptions />
  </List.ListRoot>
  <Popover.PopoverRoot>
    <Popover.PopoverContent>hi</Popover.PopoverContent>
  </Popover.PopoverRoot>
  <Shadowed />
  <Ambiguous />
</template>
