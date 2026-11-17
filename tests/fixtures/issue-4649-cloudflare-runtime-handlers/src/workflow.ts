import { WorkflowEntrypoint } from "cloudflare:workers";

export class Pipeline extends WorkflowEntrypoint {
  async run(event: unknown, step: unknown): Promise<unknown> {
    return event;
  }

  async fetch(request: Request): Promise<Response> {
    return new Response(request.url);
  }
}
