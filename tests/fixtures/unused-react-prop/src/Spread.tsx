// A NON-exported component with a rest-spread in its destructure: `rest` can
// carry any prop, so the whole component abstains. `deadInSpread` is read
// nowhere but MUST NOT be flagged because the spread makes the prop set
// statically incomplete.
const Inner = ({ tone, deadInSpread, ...rest }: Record<string, string>) => (
  <b {...rest}>{tone}</b>
);

export const Spread = ({ tone }: { tone: string }) => (
  <Inner tone={tone} deadInSpread="d" />
);
