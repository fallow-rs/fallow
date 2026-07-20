import { PlainClient } from "./plain-client";

export abstract class PlainBaseService {
  protected readonly client: PlainClient;

  constructor(client: PlainClient) {
    this.client = client;
  }
}
