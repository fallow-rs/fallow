// The registry reads the formats as one object.
import * as formats from "./formats";

const registry: Record<string, unknown> = { ...formats };
console.log(Object.keys(registry));
