import { getLeaf } from "./leaf";

export const runLeaf = (): void => {
  const leaf = getLeaf();
  leaf.inheritedUsed();
};
