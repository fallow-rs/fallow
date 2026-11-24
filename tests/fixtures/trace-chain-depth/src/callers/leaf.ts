import { value as echoed } from './echo';

export const value = 1;
export const doubled = (): number => echoed * 2;
