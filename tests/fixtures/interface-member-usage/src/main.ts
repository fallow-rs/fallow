import { FixedSizeScrollStrategy } from './fixed-size-strategy';
import {
  refreshStrategy,
  resetStrategy,
  ScrollViewport,
} from './scroll-viewport';

const strategy = new FixedSizeScrollStrategy();
const viewport = new ScrollViewport(strategy);

viewport.initialize();
viewport.destroy();
refreshStrategy(strategy);
resetStrategy(strategy);
