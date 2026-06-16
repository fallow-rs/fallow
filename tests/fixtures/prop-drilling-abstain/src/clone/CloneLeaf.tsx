// Consumer leaf for the clone chain.
export const CloneLeaf = ({ user }: { user: { name: string } }) => (
  <span>{user.name}</span>
);
