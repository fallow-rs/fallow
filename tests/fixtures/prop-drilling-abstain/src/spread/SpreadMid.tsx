import { SpreadLeaf } from "./SpreadLeaf";

// The spread (`{...user}`) on this render edge abstains the component.
export const SpreadMid = ({ user }: { user: { name: string } }) => (
  <SpreadLeaf {...user} />
);
