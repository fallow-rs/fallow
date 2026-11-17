import { DurableObject } from "cloudflare:workers";

export class ProtectedRoom extends DurableObject {
  protected async alarm(): Promise<void> {}

  protected unusedProtectedHelper(): number {
    return 1;
  }
}
