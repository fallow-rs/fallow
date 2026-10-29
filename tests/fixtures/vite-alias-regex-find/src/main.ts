import { createRequire } from "node:module";
import { log } from "virtual-logger";
import { format } from "@/format";
import { pad } from "~/pad";

log(pad(format(String(createRequire))));
