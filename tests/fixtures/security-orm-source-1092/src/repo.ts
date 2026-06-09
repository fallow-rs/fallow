import { db } from "./client";
import { run } from "./sink";

// The only source-shaped expression here is `db.query` (Drizzle), NOT an HTTP
// request source. Before #1092 this binding's `db.query` object path matched the
// `*.query` HTTP-input source, classifying repo.ts as an untrusted-source module
// that #885 then propagated onto the sink in every module it reaches (./sink).
export const find = (id: string): void => {
  const orgs = db.query.orgs;
  run(orgs.findFirst({ id }));
};
