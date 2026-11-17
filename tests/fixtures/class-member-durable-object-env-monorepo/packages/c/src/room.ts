import { DurableObject } from 'cloudflare:workers';

export class RoomC extends DurableObject {
  async viaModuleEnv(): Promise<boolean> {
    return true;
  }
}
