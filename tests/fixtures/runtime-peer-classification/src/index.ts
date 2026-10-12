import { render } from "runtime-host";
import type { Type } from "type-peer";
import type { Type as HostType } from "type-host";
import type { Type as GenuineType } from "genuine-type";
console.log(render());
export type Types = Type | HostType | GenuineType;
