import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [
    react({
      babel: {
        plugins: [["module:@preact/signals-react-transform", { mode: "auto" }]],
      },
    }),
  ],
});
