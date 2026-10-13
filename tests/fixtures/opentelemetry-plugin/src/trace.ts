import type { Sampler, SpanExporter, SpanProcessor } from "@opentelemetry/sdk-trace-base";

export class BufferExporter implements SpanExporter {
  export() {}

  shutdown() {
    return Promise.resolve();
  }

  forceFlush() {
    return Promise.resolve();
  }

  drainBuffer() {}
}

export class RateSampler implements Sampler {
  shouldSample() {
    return { decision: 1 };
  }

  toString() {
    return "RateSampler";
  }
}

export class StampProcessor implements SpanProcessor {
  onStart() {}

  onEnd() {}

  shutdown() {
    return Promise.resolve();
  }

  forceFlush() {
    return Promise.resolve();
  }
}
