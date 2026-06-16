import { SpreadMid } from "./SpreadMid";

// SPREAD abstain: the middle hop forwards via a JSX spread, so the passed-prop
// set is incomplete and the whole chain abstains.
export const SpreadPage = ({ user }: { user: { name: string } }) => (
  <SpreadMid user={user} />
);
