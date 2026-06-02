import DOMPurify from "dompurify";

// Negative: a direct DOMPurify sanitizer call suppresses the HTML sink candidate.
export function renderDefault(el: HTMLElement, userInput: string): void {
  el.innerHTML = DOMPurify.sanitize(userInput);
}
