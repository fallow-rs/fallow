export const startWorker = () => new Worker(new URL("./a.ts", import.meta.url), { type: "module" });
