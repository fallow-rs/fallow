import { execSync } from 'node:child_process';

// Forbidden in the domain zone: process execution and console logging.
export const run = (): string => {
  console.log('about to run');
  return execSync('ls').toString();
};
