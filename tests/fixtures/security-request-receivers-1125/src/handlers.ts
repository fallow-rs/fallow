import { execSync } from "node:child_process";

export const aliasBinding = (h: { query: { cmd: string } }): void => {
  const hCmd = h.query.cmd;
  execSync(`alias-binding ${hCmd}`);
};

export const aliasDirect = (httpReq: { body: { cmd: string } }): void => {
  execSync(`alias-direct ${httpReq.body.cmd}`);
};

export const builtInReceiver = (req: { params: { cmd: string } }): void => {
  const reqCmd = req.params.cmd;
  execSync(`built-in ${reqCmd}`);
};

export const ormReceiver = (db: { query: { cmd: string } }): void => {
  const dbCmd = db.query.cmd;
  execSync(`orm ${dbCmd}`);
};
