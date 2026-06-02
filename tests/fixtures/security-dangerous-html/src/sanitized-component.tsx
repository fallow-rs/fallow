import DOMPurify from "dompurify";

// Negative: React's usual __html object shape is recognized when its value is sanitized.
export function SanitizedMarkup(props: { html: string }): JSX.Element {
  return <div dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(props.html) }} />;
}
