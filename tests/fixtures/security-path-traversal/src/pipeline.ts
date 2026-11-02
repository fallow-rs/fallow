// A build pipeline step that the dev server imports. It reads no request input.
import { outputFile } from "./pipeline-output";

export const runPipeline = (count: number): string => outputFile(`chunk-${count}`);
