export const View = ({ html }: { html: string }) => { const props = { dangerouslySetInnerHTML: { __html: html } }; return <div {...props} />; };
