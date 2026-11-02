import { runInNewContext } from "node:vm";
export const run = (code: string): unknown => runInNewContext(code, {});
