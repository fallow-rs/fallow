import { ProviderMid } from "./ProviderMid";

// PROVIDER abstain: a hop renders a context `*.Provider` in its subtree, so the
// drilling may be a deliberate non-context choice and the chain abstains.
export const ProviderPage = ({ user }: { user: { name: string } }) => (
  <ProviderMid user={user} />
);
