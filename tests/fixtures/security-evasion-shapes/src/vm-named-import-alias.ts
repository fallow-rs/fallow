import { runInContext as evaluate } from "vm";
import type { Context } from "vm";
export const run = (code: string, context: Context): unknown => evaluate(code, context);
