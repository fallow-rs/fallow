import type { ReactNode } from "react";

// Consumer leaf for the element-as-prop chain.
export const ElementLeaf = ({
  user,
  slot,
}: {
  user: { name: string };
  slot: ReactNode;
}) => (
  <span>
    {user.name}
    {slot}
  </span>
);
