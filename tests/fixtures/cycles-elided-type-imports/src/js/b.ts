import { makeA } from './a.js';
export interface B { a?: ReturnType<typeof makeA> }
