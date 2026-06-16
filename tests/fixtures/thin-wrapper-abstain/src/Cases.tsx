import { forwardRef, memo, useState, createContext } from "react";
import { Base } from "./Base";

// ABSTAIN: forwardRef wrapper. Forwarding the ref is the sanctioned way to make
// a child ref-able; never redundant.
const RefWrapper = forwardRef<HTMLButtonElement, { label: string }>((props, ref) => (
  <Base {...props} ref={ref} />
));

// ABSTAIN: memo wrapper. An intentional perf boundary.
const MemoWrapper = memo((props: { label: string }) => <Base {...props} />);

// ABSTAIN: exported (public-API re-brand / encapsulation). Even a bare spread
// forward is an intentional public name.
export const PublicButton = (props: { label: string }) => <Base {...props} />;

// ABSTAIN: context provider wrapper. It provides context even while spreading.
const LabelContext = createContext("");
const ProviderWrapper = (props: { label: string }) => (
  <LabelContext.Provider value={props.label}>
    <Base {...props} />
  </LabelContext.Provider>
);

// ABSTAIN: adds a host-element wrapper (own markup). Not pure indirection.
const MarkupWrapper = (props: { label: string }) => (
  <div className="frame">
    <Base {...props} />
  </div>
);

// ABSTAIN: adds logic (a hook + branching). Not a pure passthrough.
const LogicWrapper = (props: { label: string }) => {
  const [open] = useState(false);
  return open ? <Base {...props} /> : null;
};

// ABSTAIN: a named literal attribute alongside the spread is a fixed default
// configuration, intentional.
const ConfiguredWrapper = (props: { label: string }) => (
  <Base variant="primary" {...props} />
);

// ABSTAIN: the spread root is a DIFFERENT object, not the props binding.
const OtherWrapper = (props: { label: string }) => {
  const fixed = { label: "fixed" };
  return <Base {...fixed} />;
};

// All cases are referenced so they are reachable and inspected.
export const Showcase = () => (
  <div>
    <RefWrapper label="a" />
    <MemoWrapper label="b" />
    <PublicButton label="c" />
    <ProviderWrapper label="d" />
    <MarkupWrapper label="e" />
    <LogicWrapper label="f" />
    <ConfiguredWrapper label="g" />
    <OtherWrapper label="h" />
  </div>
);
