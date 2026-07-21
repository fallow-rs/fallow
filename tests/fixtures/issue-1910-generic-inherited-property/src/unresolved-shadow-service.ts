import { DerivedClient } from "./derived-client";

export class ConcreteGrandparentService {
  constructor(protected readonly client: DerivedClient) {}
}

export class UnresolvedIntermediateService<TClient> extends ConcreteGrandparentService {
  declare protected readonly client: TClient;
}

export class UnresolvedShadowService extends UnresolvedIntermediateService<MissingClient> {
  async run(): Promise<string> {
    return this.client.shadowedByUnresolvedGeneric();
  }
}
