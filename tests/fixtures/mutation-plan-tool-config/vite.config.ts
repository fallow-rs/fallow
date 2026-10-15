import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [
    {
      name: "inline",
      transform(code: string) {
        return code.length > 10 ? code : null;
      },
    },
  ],
  test: { include: ["src/**/*.test.ts"] },
});
