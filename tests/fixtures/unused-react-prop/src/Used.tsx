// A NON-exported inner component whose `shownInner` prop is read in JSX: it must
// NOT be flagged (usage credit works through a JSX expression container).
const Inner = ({ shownInner }: { shownInner: string }) => <p>{shownInner}</p>;

export const Used = ({ shown }: { shown: string }) => (
  <Inner shownInner={shown} />
);
