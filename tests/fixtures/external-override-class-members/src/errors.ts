import { Data, Schema } from "@acme/effect";

export class NotFound extends Schema.TaggedError<NotFound>()("NotFound", { code: Schema.Number }) {
  override get message(): string {
    return "not found";
  }
  unusedHelper(): void {}
}

const sharedFields = { code: Schema.Number };

export class SpreadNotFound extends Schema.TaggedError<SpreadNotFound>()("SpreadNotFound", {
  ...sharedFields,
  detail: Schema.String,
}) {
  override get message(): string {
    return "spread not found";
  }
  spreadHelper(): void {}
}

const namedFields = { code: Schema.Number } as const;

export class NamedFieldsNotFound extends Schema.TaggedError<NamedFieldsNotFound>()(
  "NamedFieldsNotFound",
  namedFields,
) {
  override get message(): string {
    return "named fields not found";
  }
  namedFieldsHelper(): void {}
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
