import {
  AliasArgument,
  CallArgument,
  DirectRead,
  PropertyValue,
  PrototypeRead,
} from "./adapters";
import { configure, run } from "./consumer";

void run(CallArgument);
configure({ adapter: PropertyValue });
const alias = AliasArgument;
configure({ adapter: alias });
DirectRead.used();

export const direct = PrototypeRead.prototype.handlesUpgrades;
export const upgrades = PrototypeRead.upgradesTo;
