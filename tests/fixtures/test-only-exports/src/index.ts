import { usedInProd } from "./lib";

export const publicApi = (): string => usedInProd();
