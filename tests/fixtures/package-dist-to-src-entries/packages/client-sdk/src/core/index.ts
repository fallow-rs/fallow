import { coreHelper } from './helper';

export const createCoreClient = (): string => coreHelper();

export type CoreClientOptions = { url: string };
