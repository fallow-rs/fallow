import { make, makeSingle, type Sheet, type Single } from './sheet';

export type Pair = [Sheet, Single];

const sheets = make();
export const some = sheets.some((sheet) => sheet.viaCallback);
for (const sheet of sheets) sheet.viaLoop();
export const mapped = sheets.map((item) => item.viaMap());
for (const sheet of make()) sheet.viaDirectLoop();
export const first = make()[0].viaIndex();
// `makeSingle` returns one `Single`, not an array, so an element of its
// result is not a `Single`.
// @ts-expect-error `Single` is not iterable.
for (const single of makeSingle()) single.viaLoop();
