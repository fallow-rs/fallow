import * as cf from "cloudflare:workers";
import * as other from "other-runtime";

export class NsRoom extends cf.DurableObject {
  async webSocketError(ws: WebSocket, error: unknown): Promise<void> {}
}

export class NsUnknownBase extends cf.RpcTarget {
  async webSocketError(ws: WebSocket, error: unknown): Promise<void> {}
}

export class OtherNsRoom extends other.DurableObject {
  async webSocketError(ws: WebSocket, error: unknown): Promise<void> {}
}
