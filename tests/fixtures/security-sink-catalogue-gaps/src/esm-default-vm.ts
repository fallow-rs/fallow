import nodeVm from "node:vm";

export function evaluate(code: string): unknown {
  return nodeVm.runInNewContext(code);
}
