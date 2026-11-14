import type { Plugin } from "vite";

export const bannerText = (name: string): string =>
  name.length > 0 ? `/* ${name} */` : "";

export const banner = (): Plugin => ({
  name: "banner",
  transform(code) {
    return bannerText("app") + code;
  },
});
