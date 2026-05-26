import { copyFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const root = join(here, "..");

copyFileSync(join(root, "types", "index.d.ts"), join(root, "index.d.ts"));
