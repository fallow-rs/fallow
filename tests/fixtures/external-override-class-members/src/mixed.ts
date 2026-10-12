import { Mixin } from "@acme/mixins";
import * as Bases from "./base";
import { LocalBase } from "./base";

export class Mixed extends Mixin(LocalBase) {
  override run(): void {}
}

export class MixedMember extends Mixin(Bases.LocalBase) {
  override run(): void {}
}

export class MixedArray extends Mixin([LocalBase]) {
  override run(): void {}
}
