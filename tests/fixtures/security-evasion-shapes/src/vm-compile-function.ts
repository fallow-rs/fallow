const vm = require("vm");
export const compile = (code: string): unknown => vm.compileFunction(code, ["input"]);
