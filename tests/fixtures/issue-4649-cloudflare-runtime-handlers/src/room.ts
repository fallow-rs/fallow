import { DurableObject } from "cloudflare:workers";
import { DurableObject as BaseObject } from "cloudflare:workers";

export class Room extends DurableObject {
  async fetch(request: Request): Promise<Response> {
    return new Response(request.url);
  }

  async alarm(): Promise<void> {}

  async connect(socket: unknown): Promise<void> {}

  async webSocketMessage(ws: WebSocket, message: string): Promise<void> {}

  async webSocketClose(ws: WebSocket, code: number): Promise<void> {}

  async webSocketError(ws: WebSocket, error: unknown): Promise<void> {}

  async run(): Promise<void> {}

  unusedRoomHelper(): number {
    return 1;
  }
}

export class ChatRoom extends Room {
  async webSocketMessage(ws: WebSocket, message: string): Promise<void> {}
}

export class AliasRoom extends BaseObject {
  async webSocketClose(ws: WebSocket, code: number): Promise<void> {}
}
