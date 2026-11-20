import { helper, Svc } from "./internal";
export const api = (): number => helper() + new Svc().used();
export { PublicSvc, PublicMode } from "./surface";
export * from "./star";
export * as tools from "./tools";
