import { readFileSync } from "node:fs";

// A project plugin that loads `src/math.ts` itself, before the plugins
// without `enforce`. Vite uses the first `load` result that is not null.
const preLoad = {
  name: "project-pre-load",
  enforce: "pre",
  load(id) {
    return id.endsWith("/src/math.ts") ? readFileSync(id, "utf8") : null;
  },
};

export default {
  plugins: [preLoad],
  test: {
    include: ["test/math.test.ts"],
    setupFiles: ["./test/user-setup.ts"],
  },
};
