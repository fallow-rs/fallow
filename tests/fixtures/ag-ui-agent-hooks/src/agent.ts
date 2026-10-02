import { AbstractAgent } from "@ag-ui/client";

export class EchoAgent extends AbstractAgent {
  run() {
    return undefined as never;
  }

  clone() {
    return new EchoAgent();
  }

  onFinalize() {}

  formatReply() {}
}
