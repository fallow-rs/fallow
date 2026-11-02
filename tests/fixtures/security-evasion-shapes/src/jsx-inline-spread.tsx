export const View = ({ html }: { html: string }) => <div {...{ dangerouslySetInnerHTML: { __html: html } }} />;
