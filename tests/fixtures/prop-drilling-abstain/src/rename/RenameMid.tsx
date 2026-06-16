import { RenameLeaf } from "./RenameLeaf";

// The forwarded value is a CALL expression (`user.name.toUpperCase()`), not a
// plain identifier / member-root, so it sets has_complex_forward and the chain
// abstains: a transform is not a pure forward.
export const RenameMid = ({ user }: { user: { name: string } }) => (
  <RenameLeaf label={user.name.toUpperCase()} />
);
