import { C } from './c';
class Owner { readonly widget: C = new C(); }
export const createUi = () => { const factory = new Owner(); return { page: factory.widget }; };
