import { fixtureFile } from "./test-helper";

interface RequestLike {
  query: { dir: string };
}

export const run = (req: RequestLike): string => fixtureFile(req.query.dir);
