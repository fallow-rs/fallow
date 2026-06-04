// Positive: wrapping a non-literal value in a template engine SafeString marker
// emits it without HTML escaping, a template-escape-bypass candidate (CWE-79).
const Handlebars = {
  SafeString: (value: string): string => value,
};

export function unsafeMarkup(userInput: string): string {
  return Handlebars.SafeString(userInput);
}
