import { CloneMid } from "./CloneMid";

// CLONE-ELEMENT abstain: a hop calls cloneElement, which injects props by
// reflection, so the static forward-set is incomplete and the chain abstains.
export const ClonePage = ({ user }: { user: { name: string } }) => (
  <CloneMid user={user} />
);
