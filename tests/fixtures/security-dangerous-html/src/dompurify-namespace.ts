import * as DOMPurify from "dompurify";

// Negative: namespace imports from DOMPurify are recognized sanitizer provenance.
export function renderNamespace(el: HTMLElement, userInput: string): void {
  const html = DOMPurify.sanitize(userInput);
  document.write(html);
}
