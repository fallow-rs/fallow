import { fileURLToPath } from "node:url";

// A build tool loads the signer by path and calls its default export.
export const signer = fileURLToPath(new URL("./lib/signer.ts", import.meta.url));

// A script runner loads the task by path and calls its default export.
export const task = new URL("./lib/task.ts", import.meta.url).href;

// A worker module runs as a script. The main thread reads no export.
export const worker = new Worker(new URL("./lib/worker.ts", import.meta.url), {
  type: "module",
});

// A directory URL credits no export of its index module.
export const services = new URL("./services", import.meta.url);
