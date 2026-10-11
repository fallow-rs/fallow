import { C } from './c';
const makePage = (): C => new C();
export const createUi = () => ({ page: makePage() });
