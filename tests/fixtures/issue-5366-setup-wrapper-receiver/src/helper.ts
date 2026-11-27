import type { TestContext } from "./context";
import type { LocalContext } from "./local";

export const describeContexts = (a: TestContext, b: LocalContext): string => `${typeof a}${typeof b}`;
