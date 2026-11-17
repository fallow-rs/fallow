import { WorkerEntrypoint } from "cloudflare:workers";

export class Api extends WorkerEntrypoint {
  async fetch(request: Request): Promise<Response> {
    return new Response(request.url);
  }

  async scheduled(controller: unknown): Promise<void> {}

  async queue(batch: unknown): Promise<void> {}

  async email(message: unknown): Promise<void> {}

  async tail(events: unknown[]): Promise<void> {}

  async tailStream(event: unknown): Promise<void> {}

  async trace(traces: unknown[]): Promise<void> {}

  async test(controller: unknown): Promise<void> {}

  async connect(socket: unknown): Promise<void> {}

  async alarm(): Promise<void> {}
}
