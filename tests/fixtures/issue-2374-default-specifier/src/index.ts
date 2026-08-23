import { default as Aliased, named as sibling } from './alias-impl';
import { default as chained } from './chain-top';
import LocalDefault from './local-default';
import { starNamed } from './star-barrel';
import classes from './classes.module.css';
import cjsDefault from './cjs-default.cjs';

export const app = [
  Aliased,
  sibling,
  chained,
  LocalDefault,
  starNamed,
  classes.usedClass,
  cjsDefault,
];
