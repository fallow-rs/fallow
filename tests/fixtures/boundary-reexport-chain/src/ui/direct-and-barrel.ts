import { named } from '../shared';
import { named as sameNamed } from '../core/named';

export const useBoth = (): number => named + sameNamed;
