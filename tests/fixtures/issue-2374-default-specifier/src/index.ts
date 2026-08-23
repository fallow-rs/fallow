import { default as Aliased, named as sibling } from './alias-impl';
import { default as chained } from './chain-top';
import LocalDefault from './local-default';
import { starNamed } from './star-barrel';

export const app = [Aliased, sibling, chained, LocalDefault, starNamed];
