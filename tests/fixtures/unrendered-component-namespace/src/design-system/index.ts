export * as List from "./components/List";
export * as Popover from "./components/Popover";
// Dead is exposed by the barrel but never consumed by any component: its
// re-exported SFC stays reachable (the barrel is reachable) yet rendered
// nowhere, so it MUST still be flagged. The non-vacuous control.
export * as Dead from "./components/Dead";
export * from "./shadow-source";
export { default as Shadowed } from "./components/Shadow/Visible.vue";
export * from "./ambiguous-left";
export * from "./ambiguous-right";
export type { default as TypeOnly } from "./components/TypeOnly.vue";
