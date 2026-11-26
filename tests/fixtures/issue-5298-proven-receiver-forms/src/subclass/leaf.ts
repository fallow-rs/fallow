import { CoreService } from "./core";

export class LeafService extends CoreService {}

export const getLeaf = () => new LeafService();
