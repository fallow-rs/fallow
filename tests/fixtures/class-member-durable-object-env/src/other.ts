import { DurableObject } from 'cloudflare:workers';

export class Other extends DurableObject<Env> {
  async viaOtherHelper(): Promise<boolean> {
    return true;
  }

  async viaOtherField(): Promise<boolean> {
    return true;
  }
}
