import * as aliasReNs from './alias-re-barrel';

// The binding is both an object-literal alias source and exported under its
// own name: the named export hands the whole object to consumers the graph
// cannot enumerate, so the chain behind it is credited.
export const aliasReApi = { aliasReNs };
export { aliasReNs };
