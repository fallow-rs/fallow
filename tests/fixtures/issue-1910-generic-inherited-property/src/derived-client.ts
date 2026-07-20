import { BaseClient } from "./base-client";

export class DerivedClient extends BaseClient {
  // Called via `this.client.getSyntheticRecords()` where `client: TClient`
  // (TClient = DerivedClient). Must be CREDITED.
  async getSyntheticRecords(): Promise<string> {
    return "ok";
  }

  // Never called. Control - must STAY flagged.
  async deadDerivedMethod(): Promise<string> {
    return "dead";
  }
}
