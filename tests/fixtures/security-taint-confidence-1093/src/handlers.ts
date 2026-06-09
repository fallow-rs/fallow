import { execSync } from "node:child_process";

export const direct = (req: { query: { id: string } }): void => {
  const a = req.query.id;
  execSync(`run ${a}`);
};

export const viaTemplate = (req: { query: { id: string } }): void => {
  const c = `wrap-${req.query.id}`;
  execSync(`run ${c}`);
};
