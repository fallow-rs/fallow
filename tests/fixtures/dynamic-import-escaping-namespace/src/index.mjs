const loadAdapter = async (name) => {
  switch (name) {
    case "adapter":
      return import("./adapter.mjs");
    default:
      throw new Error("unknown adapter");
  }
};

function loadPlugin(name) {
  if (name === "plugin") {
    return import("./plugin.mjs");
  }
  return null;
}

const register = (value) => value;

export const start = async () => {
  const adapter = await loadAdapter(process.argv[2]);
  adapter.createAdapter();
  loadPlugin("plugin")?.then(register);
  register(import("./registered.mjs"));
  globalThis.modules = [import("./listed.mjs")];
  const pending = import("./pending.mjs");
  pending.then(register);
  const unread = import("./unread-promise.mjs");
  import("./side-effect.mjs");
  await import("./awaited-side-effect.mjs");
  import("./caught-side-effect.mjs").catch(() => {});
  import("./then-side-effect.mjs").then(() => {});
};
