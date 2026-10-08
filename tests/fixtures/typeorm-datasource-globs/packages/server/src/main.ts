import { connectionSource } from './database/core.datasource';

export const start = async (): Promise<void> => {
  await connectionSource.initialize();
};
