import { DurableObject } from "cloudflare:workers";

class SpecifierRoom extends DurableObject {
  async alarm(): Promise<void> {}
}

export { SpecifierRoom };
