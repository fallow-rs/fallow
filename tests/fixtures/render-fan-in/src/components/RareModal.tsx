// Rendered in exactly ONE site (Settings): the rarely-rendered baseline that
// must NOT appear as high-fan-in. Expected render-sites = 1, distinct-parents = 1.
export const RareModal = (props: { open: boolean }) => {
  return props.open ? <div>modal</div> : null;
};
