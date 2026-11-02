// A dev server that reads `req.url` and imports a build pipeline.
import { runPipeline } from "./pipeline";

export const onRequest = (req: { url: string }): string => runPipeline(req.url.length);
