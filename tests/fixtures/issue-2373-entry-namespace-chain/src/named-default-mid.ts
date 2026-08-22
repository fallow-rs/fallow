// The rename lands on `default`, which the entry's plain `export *` leaves
// behind, so the namespace object stays off the entry surface.
export { defaultNs as default } from './named-default-barrel';
