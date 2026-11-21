import { chainUsed, deepUsed, ns, used, type UsedType } from "@p/lib";
import { subUsed } from "@p/lib/sub";
import { plainUsed } from "@p/plain";

const value: UsedType = { id: "a" };
console.log(used, chainUsed, deepUsed, ns.nsMember, plainUsed, subUsed, value);
