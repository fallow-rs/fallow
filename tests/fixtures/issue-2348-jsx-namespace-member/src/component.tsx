import * as SC from './style';
import { RenderedCard } from './card';
import { BarrelButton } from './barrel-consumer';

export const RenderedStyle = () => (
  <div>
    <SC.UsedStyle />
    <RenderedCard />
    <BarrelButton />
  </div>
);
