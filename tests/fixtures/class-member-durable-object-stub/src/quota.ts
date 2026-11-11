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
}
