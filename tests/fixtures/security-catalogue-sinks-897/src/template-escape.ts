// Positive: wrapping a non-literal value in a template engine SafeString marker
// emits it without HTML escaping, a template-escape-bypass candidate (CWE-79).
// The stub runs a call, so the name match applies. A local method that runs
// no call cannot reach the library, so it is not a sink.
const Handlebars = {
  SafeString: (value: string): string => String(value),
};

export function unsafeMarkup(userInput: string): string {
  return Handlebars.SafeString(userInput);
}
