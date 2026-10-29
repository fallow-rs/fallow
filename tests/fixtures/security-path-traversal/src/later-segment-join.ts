// Positive (#3712): a dynamic later `path.join` segment must fire.
import * as path from "node:path";

export const joinSecond = (input: string): string => path.join(".", input);
