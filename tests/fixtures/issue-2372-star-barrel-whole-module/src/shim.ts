import * as shimNs from './shim-barrel';

// Whole-object use in a reachable non-entry module.
export const shimAll = Object.keys(shimNs);
