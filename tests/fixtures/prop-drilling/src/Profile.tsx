// CONSUMER hop: `user` is substantively read here (outside any forwarded attr),
// terminating the drilling chain.
export const Profile = ({ user }: { user: { name: string } }) => (
  <span>{user.name}</span>
);
