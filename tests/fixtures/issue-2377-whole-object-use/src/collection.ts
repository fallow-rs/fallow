import * as ArrayIcons from './array-icons';
import * as ObjectIcons from './object-icons';

const registry = [ArrayIcons];
const lookup = { icons: ObjectIcons };

export const collected = (): unknown[] => [
  ArrayIcons.ArrayStar(),
  ObjectIcons.ObjectStar(),
  registry,
  lookup,
];
