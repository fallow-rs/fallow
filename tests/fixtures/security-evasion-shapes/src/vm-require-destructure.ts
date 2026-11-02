const { runInThisContext } = require("vm");
export const run = (code: string): unknown => runInThisContext(code);
