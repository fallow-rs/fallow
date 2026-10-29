// Positive (#3712): two dynamic segments in one call produce one candidate.
import * as path from "node:path";

export const joinBoth = (base: string, input: string): string => path.join(base, input);
