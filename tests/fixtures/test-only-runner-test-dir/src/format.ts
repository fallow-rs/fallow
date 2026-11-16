import { camelCase } from 'lodash';

export const format = (value: string): string => camelCase(value);
