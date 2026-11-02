// Negative: only a test file imports this helper. A request read in a test
// file is not production input, so it does not make this module reachable.
import * as path from "node:path";

export const fixtureFile = (dir: string): string => path.join(dir, "fixture.json");
