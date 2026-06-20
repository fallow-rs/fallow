<script setup lang="ts">
import * as DS from "@/design-system";
// Side-effect import keeps the widgets barrel reachable without rendering its
// Orphan re-export: the non-vacuous control.
import "@/widgets";
</script>

<template>
  <DS.List.ListRoot>
    <DS.List.ListItem />
  </DS.List.ListRoot>
  <DS.Popover.PopoverRoot>
    <DS.Popover.PopoverContent />
  </DS.Popover.PopoverRoot>
</template>
