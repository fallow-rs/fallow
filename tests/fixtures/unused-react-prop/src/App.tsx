import { Local } from "./Local";
import { Exported } from "./Exported";
import { ForwardRef } from "./ForwardRef";
import { Spread } from "./Spread";
import { Nested } from "./Nested";
import { Used } from "./Used";
import { Typed } from "./Typed";
import { ForwardRefGeneric } from "./ForwardRefGeneric";

// `App` is exported (public contract): its props abstain. It also renders every
// child so each child module is reachable.
export const App = () => (
  <div>
    <Local title="t" />
    <Exported label="l" />
    <ForwardRef caption="c" />
    <Spread tone="warm" />
    <Nested user={{ name: "a" }} />
    <Used shown="s" />
    <Typed heading="h" />
    <ForwardRefGeneric heading="h" />
  </div>
);
