import { execSync } from "node:child_process";

export const refundLimit = (): number =>
  Number(execSync("cat refund-limit.txt").toString());
