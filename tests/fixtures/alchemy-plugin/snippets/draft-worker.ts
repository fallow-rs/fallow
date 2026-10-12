import * as Cloudflare from "alchemy/Cloudflare";

export default class Draft extends Cloudflare.Worker<Draft>()("Draft", {
  main: import.meta.url,
}) {}
