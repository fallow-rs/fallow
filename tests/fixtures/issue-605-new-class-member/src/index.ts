import { URL } from "./builtin-named";
import { OptionBuilder } from "./option-builder";
import { TracesRepository } from "./repo";

declare const client: unknown;

// Direct constructor-receiver calls (everr#144 shape). These credit
// `TracesRepository.search` and `TracesRepository.getTrace`.
export function runRepo(): string {
  const found = new TracesRepository(client).search("query");
  const trace = new TracesRepository(client).getTrace("trace-id");
  return `${found}:${trace}`;
}

// Fluent chain rooted at `new OptionBuilder()` (graphql-markdown#2949 shape).
// Credits addDefault (direct), addFromConfig / addFromCli / build (chain).
export function buildConfig(): string | undefined {
  return new OptionBuilder<string>()
    .addDefault("default", "key")
    .addFromConfig("config", "key")
    .addFromCli("cli", "key")
    .build();
}

// Negative case: `peek()` is NOT self-returning, so the downstream
// `afterPeek()` emits a new-root fluent sentinel that the analyze-layer guard
// must reject (the chain has left the OptionBuilder type at `peek()`).
// `afterPeek` must STAY reported as unused. `peek` itself is credited because
// it is the first method directly off the constructor. Static analysis does
// not typecheck this call, so the unsound `peek().afterPeek()` shape is fine.
export function leavesTheType(): void {
  // @ts-expect-error intentional: exercises the non-self-returning chain guard
  new OptionBuilder<string>().peek().afterPeek();
}

// A user class whose name collides with a global builtin. `URL.parse` must be
// credited (the analyze layer resolves `URL` to the user export), proving the
// extraction layer does not drop builtin-shaped names. `URL.unusedOnUrl` stays
// flagged. Regression for Codex's review of issue #605.
export function usesBuiltinNamedClass(): string {
  return new URL().parse();
}
