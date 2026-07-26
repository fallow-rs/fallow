type ApiResult = import("./barrel.js").PublicComplex<
  import("./barrel.js").PublicApi<string>
>;
type MergedModule = typeof import("./barrel.js").PublicMerged;

export const result: ApiResult = "used";
export const kind: MergedModule["kind"] = "merged";

void import("./barrel.js").then(({ RuntimeOnly }) => RuntimeOnly);
