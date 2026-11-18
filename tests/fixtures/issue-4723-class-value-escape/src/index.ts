import {
  AliasArgument,
  CallArgument,
  DirectRead,
  HolderRead,
  KeyedRead,
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

const transports = { websocket: HolderRead, keyed: KeyedRead };
export const computed = (name: "websocket"): boolean =>
  transports[name].prototype.handlesUpgrades;
export const keyed = transports.keyed.prototype.keyed;
// @ts-expect-error: the read through another key does not use `missed`.
export const mismatch = transports.websocket.prototype.missed;
