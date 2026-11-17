import { DurableObject } from "cloudflare:workers";

class LocalBase extends DurableObject {}

export class LocalChainRoom extends LocalBase {
  async alarm(): Promise<void> {}

  unusedLocalChainHelper(): number {
    return 1;
  }
}

class GenericBase<E> extends DurableObject<E> {}

export class GenericChainRoom extends GenericBase<Env> {
  async alarm(): Promise<void> {}
}

class InnerRoom extends LocalBase {
  async webSocketClose(ws: WebSocket, code: number): Promise<void> {}
}

export { InnerRoom as RenamedRoom };

export default class DefaultChainRoom extends LocalBase {
  async alarm(): Promise<void> {}
}
