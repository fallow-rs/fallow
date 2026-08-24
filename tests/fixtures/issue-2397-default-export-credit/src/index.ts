import { thing } from 'ambient-star';
import A from './a';
import B from './b';
import theme from './theme.cjs';
import computed from './computed.cjs';
import compound from './compound.cjs';
import handoff from './handoff.cjs';
import { default as aliasedMember } from './aliased-member.cjs';
import { default as aliasedHandoff } from './aliased-handoff.cjs';
import shadowed from './shadowed.cjs';

const hand = (value: unknown): unknown => value;
const previewShadowed = (shadowed: { shadowSpare: string }): unknown[] => [
  hand(shadowed),
  shadowed.shadowSpare,
];

export const app = [
  thing,
  A,
  B,
  theme.primary,
  computed.computedUsed,
  compound.compoundUsed,
  handoff.handoffUsed,
  hand(handoff),
  aliasedMember.aliasMemberUsed,
  aliasedHandoff.aliasHandoffUsed,
  hand(aliasedHandoff),
  shadowed.shadowUsed,
  previewShadowed,
];
