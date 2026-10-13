import type { TextMapPropagator } from "@opentelemetry/api";
import { MetricReader } from "@opentelemetry/sdk-metrics";

export class HeaderPropagator implements TextMapPropagator {
  inject() {}

  extract(context: unknown) {
    return context;
  }

  fields() {
    return ["x-trace"];
  }
}

// The hooks have no `protected` modifier, so the analysis tracks them.
export class PollReader extends MetricReader {
  onInitialized() {}

  onForceFlush() {
    return Promise.resolve();
  }

  onShutdown() {
    return Promise.resolve();
  }

  pollInterval() {
    return 1000;
  }
}
