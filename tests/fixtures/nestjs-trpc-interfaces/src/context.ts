import type { TRPCContext } from "nestjs-trpc";

export class AppContext implements TRPCContext {
  create() {
    return {};
  }

  helper() {}
}
