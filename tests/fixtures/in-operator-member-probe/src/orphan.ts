import { UnrelatedReport } from "./unrelated";

export const orphanDetail = (cause: unknown): boolean =>
  cause instanceof UnrelatedReport && "displayDetail" in cause;
