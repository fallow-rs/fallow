// The store lives on the global object, so it outlives a module that the
// test runner loads again.
export const store: { value?: number } = ((
  globalThis as { fallowStore?: { value?: number } }
).fallowStore ??= {});

export function reset(): void {
  delete store.value;
}
