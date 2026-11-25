import { usedThroughBarrel } from "./barrel";
import { usedInProd } from "./lib";

export const publicApi = (): string => usedInProd() + usedThroughBarrel();
