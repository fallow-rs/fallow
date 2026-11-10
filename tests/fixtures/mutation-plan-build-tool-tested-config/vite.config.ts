import { defineConfig } from "vite";
import { prefix } from "./vite-plugins/helper";

export default defineConfig({
  plugins: [
    {
      name: "inline",
      transform(code: string) {
        return code.length > 10 ? prefix(code) : null;
      },
    },
  ],
});
