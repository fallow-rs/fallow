import { Data, Schema } from "@acme/effect";

export class NotFound extends Schema.TaggedError<NotFound>()("NotFound", { code: Schema.Number }) {
  override get message(): string {
    return "not found";
  }
  unusedHelper(): void {}
}

export class Failure extends Data.TaggedError("Failure")<{ readonly code: number }> {
  override toString(): string {
    return "failure";
  }
}

export class PlainFailure extends Error {
  override get message(): string {
    return "plain";
  }
  unusedHelper(): void {}
}

export class LooseFailure extends Error {
  toString(): string {
    return "loose";
  }
  get cause(): unknown {
    return undefined;
  }
}
