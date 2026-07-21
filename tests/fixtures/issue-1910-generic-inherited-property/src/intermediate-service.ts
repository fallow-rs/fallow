import { BaseClient } from "./base-client";
import { BaseService } from "./base-service";

export abstract class IntermediateService<
  TClient extends BaseClient,
> extends BaseService<TClient> {}
