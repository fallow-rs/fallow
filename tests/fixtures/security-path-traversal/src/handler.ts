// A request handler that passes request input to a helper module.
import { joinUnderRoot } from "./imported-by-handler";

export const handle = (req: { url: string }): string => joinUnderRoot(req.url);
