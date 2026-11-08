// A registry that rejects a second registration, like a metrics or
// custom-element registry. It keeps its state on a built-in object that the
// worker state check does not read, so only a second load in one process
// shows the failure.
const registry = Error as unknown as { fallowRegistered?: boolean };
if (registry.fallowRegistered) throw new Error("requests is already registered");
registry.fallowRegistered = true;

export const registered = (): number => 1;
