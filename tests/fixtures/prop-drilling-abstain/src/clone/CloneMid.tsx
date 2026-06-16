import { cloneElement } from "react";
import { CloneLeaf } from "./CloneLeaf";

// cloneElement in the body abstains this component (props by reflection).
export const CloneMid = ({ user }: { user: { name: string } }) => {
  const el = <CloneLeaf user={user} />;
  return cloneElement(el, { extra: true });
};
