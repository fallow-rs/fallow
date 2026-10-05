import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  integrations: [
    starlight({
      title: "Docs",
      components: {
        Header: "./src/components/Header.astro",
      },
      customCss: ["./src/styles/custom.css", "@fontsource/inter"],
    }),
  ],
});
