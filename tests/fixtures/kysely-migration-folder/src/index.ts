import { migrator } from './repositories/database.js';

export const start = () => migrator();
