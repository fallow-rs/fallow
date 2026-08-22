import * as reNs from './re-barrel';

// A namespace binding re-exported from a non-entry module reaches consumers
// the graph cannot enumerate, so it credits the whole namespace object.
export { reNs };
