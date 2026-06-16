import { Button } from "./Button";

// THIN WRAPPER: a non-exported local component whose entire body forwards its
// own props to a single child via a bare spread. No host wrapper, no named
// attrs, no hooks, no branching. A candidate for inlining at its call site.
const Wrapper = (props: { label: string }) => <Button {...props} />;

// Entry point: renders the wrapper, so it is reachable but adds no value.
export const App = () => <Wrapper label="Save" />;
