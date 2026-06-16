// `Local` is exported (abstains) and renders a NON-exported inner component
// whose `deadProp` is destructured but read nowhere: the true positive.
// `kept` is read in the body and must NOT be flagged.
const LocalInner = ({ deadProp, kept }: { deadProp: string; kept: string }) => (
  <span>{kept}</span>
);

export const Local = ({ title }: { title: string }) => (
  <section>
    {title}
    <LocalInner deadProp="x" kept="y" />
  </section>
);
