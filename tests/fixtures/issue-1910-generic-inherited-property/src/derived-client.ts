import { BaseClient } from "./base-client";

export class DerivedClient extends BaseClient {
  // Called via `this.client.getSyntheticRecords()` where `client: TClient`
  // (TClient = DerivedClient). Must be CREDITED.
  async getSyntheticRecords(): Promise<string> {
    return "ok";
  }

  // Called through `DeepDerivedService -> IntermediateService<T> ->
  // BaseService<T>`. The concrete type argument must survive both hops.
  async getDeepRecords(): Promise<string> {
    return "deep";
  }

  // Called through a field whose nearer generic declaration cannot resolve.
  // The same-named concrete grandparent field must remain shadowed, so this
  // method must NOT be credited.
  async shadowedByUnresolvedGeneric(): Promise<string> {
    return "shadowed";
  }

  // Never called. Control - must STAY flagged.
  async deadDerivedMethod(): Promise<string> {
    return "dead";
  }
}
