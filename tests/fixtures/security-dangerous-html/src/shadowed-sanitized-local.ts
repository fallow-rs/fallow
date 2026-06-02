import DOMPurify from "dompurify";

// Positive: a shadowing parameter must not inherit the outer sanitized binding.
export function renderShadowed(el: HTMLElement, userInput: string): void {
  const html = DOMPurify.sanitize(userInput);

  function write(html: string): void {
    el.innerHTML = html;
  }

  write(userInput);
  void html;
}
