import * as aliasNs from './alias-barrel';

// A namespace binding placed in an exported object literal: consumers reach
// it as `aliasApi.aliasNs.<member>`, which the alias phase follows precisely.
export const aliasApi = { aliasNs };
