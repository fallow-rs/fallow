import { DurableObject } from 'cloudflare:workers';

export class Room extends DurableObject<Env> {
  async viaFetchEnv(): Promise<boolean> {
    return true;
  }

  async viaQueueEnv(): Promise<boolean> {
    return true;
  }

  async viaPlainObject(): Promise<boolean> {
    return true;
  }

  async unused(): Promise<boolean> {
    return true;
  }
}
