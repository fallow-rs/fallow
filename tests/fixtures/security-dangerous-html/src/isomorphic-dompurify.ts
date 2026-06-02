import DOMPurify from "isomorphic-dompurify";

// Negative: the isomorphic DOMPurify package has the same trusted API shape.
export function renderIsomorphic(el: HTMLElement, userInput: string): void {
  el.innerHTML = DOMPurify.sanitize(userInput);
}
