import { DurableObject } from 'cloudflare:workers';

export class DailyQuota extends DurableObject {
  async viaGet(n: number): Promise<boolean> {
    return n > 0;
  }

  async viaGetByName(n: number): Promise<boolean> {
    return n > 1;
  }

  async viaIdLocal(n: number): Promise<boolean> {
    return n > 2;
  }

  async viaAnnotatedStub(n: number): Promise<boolean> {
    return n > 3;
  }

  async unused(): Promise<void> {}

  // The Worker calls these names on the namespace, not on a stub.
  async get(): Promise<void> {}

  async getByName(): Promise<void> {}

  async idFromName(): Promise<void> {}

  // The Worker calls this name on a stub, so the call runs this method.
  async idFromString(): Promise<boolean> {
    return true;
  }
}
