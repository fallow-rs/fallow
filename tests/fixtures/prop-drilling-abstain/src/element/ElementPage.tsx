import { ElementMid } from "./ElementMid";

// ELEMENT-AS-PROP abstain: the chain passes a JSX element / component as a prop
// value (`component={<X/>}` indirection), a complex forward the chain abstains.
export const ElementPage = ({ user }: { user: { name: string } }) => (
  <ElementMid user={user} />
);
