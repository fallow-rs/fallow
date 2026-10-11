import { Owner } from './owner';
export const createUi = () => { const factory = new Owner(); return { page: factory.widget }; };
