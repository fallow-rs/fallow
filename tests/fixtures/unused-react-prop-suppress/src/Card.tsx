// `Inner` is non-exported with an unused `intentional` prop, but the inline
// suppression token directly above the component declaration drops the finding.
// The comment line sits directly above the prop's anchor line (the destructure
// signature), so `fallow-ignore-next-line` covers it.
// fallow-ignore-next-line unused-component-prop
const Inner = ({ intentional, shown }: { intentional: string; shown: string }) => (
  <em>{shown}</em>
);

export const Card = ({ label }: { label: string }) => (
  <Inner intentional="i" shown={label} />
);
