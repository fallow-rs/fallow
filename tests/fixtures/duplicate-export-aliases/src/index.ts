import layer from "./worker";
import { a, b } from "./specifiers";
import def from "./default-alias";
import { q2 } from "./re-exports";
import { one, two } from "./same-value";
import * as merged from "./type-and-value";
import { kept } from "./suppressed";
import { foo } from "./named-default-function";
import { only } from "./single";
import cjs from "./cjs";
import { pub } from "./ignored";
import { Kind } from "./merged-name";
import { r2 } from "./forward";
import type { LegacyProps } from "./interface-alias";
import { r3 } from "./re-export-specifiers";

console.log(layer, a, b, def, q2, one, two, merged, kept, foo, only, cjs, pub, Kind, r2, r3);
export type { LegacyProps };
