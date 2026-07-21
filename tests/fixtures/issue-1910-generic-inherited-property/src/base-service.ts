import { BaseClient } from "./base-client";

export abstract class BaseService<TClient extends BaseClient> {
  protected readonly client: TClient;

  constructor(client: TClient) {
    this.client = client;
  }
}
