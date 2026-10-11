import { C } from './c';
const makePage = (): C => new C();
export const createUi = () => { const page = makePage(); return { page }; };
