import { ElementLeaf } from "./ElementLeaf";

// The `slot` attr value is a JSX element (element-as-prop indirection), which
// sets has_complex_forward and abstains the component's render edge.
export const ElementMid = ({ user }: { user: { name: string } }) => (
  <ElementLeaf user={user} slot={<div>injected</div>} />
);
