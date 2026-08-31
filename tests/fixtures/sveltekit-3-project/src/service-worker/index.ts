import { build } from "$app/service-worker";

globalThis.addEventListener("install", () => {
  globalThis.console.log(build.length);
});
