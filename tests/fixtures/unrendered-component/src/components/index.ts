// Internal barrel. Re-exports keep every component reachable and "export-used",
// masking the fact that `Orphan` is rendered nowhere in the project.
export { default as Used } from "./Used.vue";
export { default as ExplicitDefault } from "./ExplicitDefault.vue";
export {
  default as NamedExportOrphan,
  helper as NamedOnly,
} from "./NamedExportOrphan.vue";
export { default as Orphan } from "./Orphan.vue";
export { default as Lazy } from "./Lazy.vue";
// Options-API components: an explicit `export default` in the `<script>` block.
// `UsedOptions` is rendered, `OrphanOptions` is not.
export { default as UsedOptions } from "./UsedOptions.vue";
export { default as OrphanOptions } from "./OrphanOptions.vue";
