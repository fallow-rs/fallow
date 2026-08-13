import type { Foo } from "./barrel.js";

export const render = (foo: Foo): string => String(foo);
