import { used, Status, Service } from "./utils";
import { fromBarrel } from "./barrel";
import { flagA, flagB } from "./flags";

console.log(used, Status.Active, new Service().start(), fromBarrel, flagA, flagB);
