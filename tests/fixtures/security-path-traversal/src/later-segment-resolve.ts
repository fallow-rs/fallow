// Positive (#3712): a literal first segment does not contain a dynamic later
// segment, so the later argument must fire.
import * as path from "node:path";

export const resolveSecond = (input: string): string => path.resolve(".", input);
