import { width } from './a';
export interface Box { width: number }
export const measure = (box: Box): number => width(box);
