import { FixedSizeScrollStrategy } from './fixed-size-strategy';
import {
  inspectStrategy,
  refreshStrategy,
  resetStrategy,
  StrategyRegistry,
  StrategyContext,
  ScrollViewport,
} from './scroll-viewport';

const strategy = new FixedSizeScrollStrategy();
const viewport = new ScrollViewport(strategy);

viewport.initialize();
viewport.destroy();
refreshStrategy(strategy);
resetStrategy(strategy);
inspectStrategy(new StrategyContext(strategy));
new StrategyRegistry(strategy).flush();
