import { defineConfig } from "vite-plus";

export default defineConfig(({ command }) => {
  const watching = command === "serve";
  return {
    build: {
      outDir: "dist",
      ...(watching ? { watch: { include: "src/**" } } : {}),
      rolldownOptions: {
        input: ["index.html", "second.html"],
        output: { entryFileNames: "[name].js" },
      },
    },
    environments: {
      worker: {
        build: {
          rolldownOptions: { input: "src/workerMain.ts" },
        },
      },
    },
  };
});
