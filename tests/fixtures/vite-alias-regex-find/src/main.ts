import { createRequire } from "node:module";
import { log } from "virtual-logger";
import { format } from "@/format";

log(format(String(createRequire)));
