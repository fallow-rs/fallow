import type { JSX } from './types';
import { merged } from './types';

export const render = (element: JSX.Element): string => element.tag + merged();
