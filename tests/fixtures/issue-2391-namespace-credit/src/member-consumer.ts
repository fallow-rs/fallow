import * as Members from './member-target';

const hand = (value: unknown): unknown => value;

export const useNamespaceMembers = (): unknown => {
  void Members.NamespaceStar;
  return hand(Object.values(Members));
};
