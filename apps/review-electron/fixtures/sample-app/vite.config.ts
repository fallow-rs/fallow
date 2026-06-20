import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev mode keeps the automatic JSX runtime in development, so each element
// carries jsx-source metadata (_source: fileName/lineNumber) that the W5
// grounded-inspector picker reads from the React fiber.
export default defineConfig({
  plugins: [react()],
  server: { port: 5273 },
});
