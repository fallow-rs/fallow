import vm from "node:vm";
export const compile = (code: string): vm.Script => new vm.Script(code);
