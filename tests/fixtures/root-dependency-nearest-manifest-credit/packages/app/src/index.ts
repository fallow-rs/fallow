import { fallback } from "fallback-lib";
import { peer } from "peer-lib";
import { shared } from "shared-runtime";

export const app = [fallback(), peer(), shared()];
