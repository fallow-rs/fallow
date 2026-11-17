import { DurableObject } from "cloudflare:workers";

class LocalBase extends DurableObject {}

export class LocalChainRoom extends LocalBase {
  async alarm(): Promise<void> {}

  unusedLocalChainHelper(): number {
    return 1;
  }
}
