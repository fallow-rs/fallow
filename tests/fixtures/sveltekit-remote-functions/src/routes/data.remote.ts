import { query, form } from '$app/server';

// Imported and called from +page.svelte.
export const getData = query(async () => {
  return { count: 42 };
});

// Exported but invoked only through SvelteKit's generated form binding, never
// imported directly. Must not surface as an unused export.
export const submitData = form(async (formData: FormData) => {
  return { ok: formData.has('value') };
});
