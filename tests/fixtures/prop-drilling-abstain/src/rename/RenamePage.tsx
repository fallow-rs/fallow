import { RenameMid } from "./RenameMid";

// RENAMED-TRANSFORM abstain: a hop transforms the prop (a call expression) before
// forwarding, so the forwarded value is not the received identifier's root and
// the chain abstains (a transform, not a pure forward).
export const RenamePage = ({ user }: { user: { name: string } }) => (
  <RenameMid user={user} />
);
