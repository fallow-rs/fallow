import { DurableObject as OtherObject } from "other-runtime";

class DurableObject {
  base(): number {
    return 1;
  }
}

export class FakeRoom extends DurableObject {
  async webSocketMessage(ws: WebSocket, message: string): Promise<void> {}
}

export class OtherRoom extends OtherObject {
  async webSocketMessage(ws: WebSocket, message: string): Promise<void> {}
}

export class PlainRoom {
  async webSocketClose(ws: WebSocket, code: number): Promise<void> {}
}
