import { Package as NativePackage } from "../native.js";

export * from "../native.js";

export class Package extends NativePackage {
  build() {
    return this.load() + 1;
  }
}
