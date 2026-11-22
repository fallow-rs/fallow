import { Reexported } from "./reexport";
import { Named } from "./barrel/named";

export const consumerValues: unknown[] = [Reexported.reexportedUsed, Object.keys(Named)];
