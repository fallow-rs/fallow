import { Data } from "effect";

export class NotFound extends Data.TaggedError("NotFound")<{ readonly id: string }> {}

// fallow-ignore-next-line unused-tagged-error
export class Expired extends Data.TaggedError("Expired")<{}> {}
