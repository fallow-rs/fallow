// Same shape as `src/lib/util.ts`, on a path whose override turns
// `private-type-leaks` off.
type UiInternal = { id: string };

const makeUi = (): UiInternal => ({ id: 'ui' });

export const buildUi = () => makeUi();
