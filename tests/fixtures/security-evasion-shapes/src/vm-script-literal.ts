import vm from "node:vm";
export const compile = (): vm.Script => new vm.Script("1 + 1");
