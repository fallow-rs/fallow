import { Data } from "effect";

export class NotFound extends Data.TaggedError("NotFound")<{ readonly id: string }> {}

export class Expired extends Data.TaggedError("Expired")<{}> {}
