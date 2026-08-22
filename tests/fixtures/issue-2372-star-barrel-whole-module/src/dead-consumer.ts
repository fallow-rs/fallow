import * as deadNs from './dead-barrel';

// No entry point reaches this file: the report already calls it unused, so
// its whole-object use must not credit the barrel's chain and turn the dead
// subtree into unused-export rows underneath the unused-file rows.
export const deadAll = Object.values(deadNs);
