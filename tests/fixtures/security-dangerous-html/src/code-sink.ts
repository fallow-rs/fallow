import DOMPurify from "dompurify";

// Positive: HTML sanitization must not suppress non-HTML sink families.
export function runCode(userInput: string): void {
  eval(DOMPurify.sanitize(userInput));
}
