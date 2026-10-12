import * as Direct from "./direct";
import * as Barrel from "./barrel";
import { Grouped } from "./grouped";

export function main(): unknown[] {
  return [Direct.Registry.layer, Barrel.Service.live, Grouped.Client.layer, Direct.Mode.Read];
}

export function shadowed(Direct: { Registry: { other: number } }): number {
  return Direct.Registry.other;
}

export function shadowedNamed(Grouped: { Client: { testLayer: number } }): number {
  return Grouped.Client.testLayer;
}
