import { DurableObject } from 'cloudflare:workers';

export class Room extends DurableObject<Env> {
  async viaEnvImport(): Promise<boolean> {
    return true;
  }

  async viaEnvImportInline(): Promise<boolean> {
    return true;
  }

  async viaThisEnv(): Promise<boolean> {
    return true;
  }

  async viaThisEnvLocal(): Promise<boolean> {
    return true;
  }

  async viaParam(): Promise<boolean> {
    return true;
  }

  async viaHelper(): Promise<boolean> {
    return true;
  }

  async viaHelperLocal(): Promise<boolean> {
    return true;
  }

  async viaField(): Promise<boolean> {
    return true;
  }

  // Each caller below reads this name on a receiver that is not a `Room`.
  async viaFakeEnv(): Promise<boolean> {
    return true;
  }

  async viaPlainThis(): Promise<boolean> {
    return true;
  }

  async viaOtherHelper(): Promise<boolean> {
    return true;
  }

  async viaOtherField(): Promise<boolean> {
    return true;
  }

  async unused(): Promise<void> {}
}
