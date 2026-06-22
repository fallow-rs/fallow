// Barrel re-exporting both components by default. `Used` is rendered by the
// page; `Orphan` is kept reachable only by this barrel and rendered nowhere.
export { default as Used } from './Used.astro';
export { default as Orphan } from './Orphan.astro';
