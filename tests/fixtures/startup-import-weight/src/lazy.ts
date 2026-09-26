import { shared } from './shared';
import { lazyOnly } from './lazy-only';

export const lazy = () => [shared, lazyOnly];
