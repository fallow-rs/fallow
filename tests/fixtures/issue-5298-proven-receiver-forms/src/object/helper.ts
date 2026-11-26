import type { TestContext } from "./context";

export const describeContext = (ctx: TestContext): string => typeof ctx;
