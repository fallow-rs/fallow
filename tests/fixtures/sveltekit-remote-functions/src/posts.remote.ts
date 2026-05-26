import { query, command } from '$app/server';

// Generated-binding-only remote file: no other module imports it. SvelteKit
// reaches these exports through client/server bindings fallow cannot see, so
// neither the file nor its exports should report as unused.
export const getPosts = query(async () => {
  return [{ id: 1, title: 'Hello' }];
});

export const addPost = command(async (title: string) => {
  return { id: 2, title };
});
