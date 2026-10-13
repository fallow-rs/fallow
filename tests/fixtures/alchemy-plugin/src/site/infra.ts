import * as Cloudflare from "alchemy/Cloudflare";

export const Site = Cloudflare.Worker("Site", {
  main: new URL("./entry.ts", import.meta.url).pathname,
});
