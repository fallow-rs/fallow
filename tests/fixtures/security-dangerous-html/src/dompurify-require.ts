const DOMPurify = require("dompurify");

// Negative: CommonJS require bindings from DOMPurify are recognized too.
export function renderRequire(el: HTMLElement, userInput: string): void {
  const html = DOMPurify.sanitize(userInput);
  el.insertAdjacentHTML("beforeend", html);
}
