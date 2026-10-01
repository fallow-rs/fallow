// The same shape as `unused-react-prop/src/Local.tsx`, where `deadProp` is
// flagged. This project declares no React runtime, so nothing is flagged here.
const LocalInner = ({ deadProp, kept }: { deadProp: string; kept: string }) => (
  <span>{kept}</span>
);

export const Local = ({ title }: { title: string }) => (
  <section>
    {title}
    <LocalInner deadProp="x" kept="y" />
  </section>
);
