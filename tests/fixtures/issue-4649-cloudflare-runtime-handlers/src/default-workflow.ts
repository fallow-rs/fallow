import { WorkflowEntrypoint } from "cloudflare:workers";

export default class extends WorkflowEntrypoint {
  async run(event: unknown, step: unknown): Promise<unknown> {
    return event;
  }

  async unusedWorkflowStep(): Promise<void> {}
}
