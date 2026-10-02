import type { TRPCErrorHandler } from "nestjs-trpc";

export class AppErrorHandler implements TRPCErrorHandler {
  onError() {}
}
