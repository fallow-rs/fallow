import type { Plugin } from "vite";

export const stampText = (version: number): string =>
  version > 1 ? `v${version}` : "v1";

export const stamp = (): Plugin => ({
  name: "stamp",
  transform(code) {
    return code + stampText(2);
  },
});
