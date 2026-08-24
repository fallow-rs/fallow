const hand = (value: unknown): unknown => value;
const Icons = require('./require-icons');

export const useRequireObject = (): unknown => hand(Icons) ?? Icons.RequireStar;
