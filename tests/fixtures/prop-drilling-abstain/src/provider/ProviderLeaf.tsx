// Consumer leaf for the provider chain.
export const ProviderLeaf = ({ user }: { user: { name: string } }) => (
  <span>{user.name}</span>
);
