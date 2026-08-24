import { thing } from 'ambient-star';
import A from './a';
import B from './b';
import theme from './theme.cjs';
import computed from './computed.cjs';
import compound from './compound.cjs';
import handoff from './handoff.cjs';
import { default as aliasedMember } from './aliased-member.cjs';
import { default as aliasedHandoff } from './aliased-handoff.cjs';

const hand = (value: unknown): unknown => value;

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
];
