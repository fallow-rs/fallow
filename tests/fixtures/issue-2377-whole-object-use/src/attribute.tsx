import * as AttributeIcons from './attribute-icons';
import { Callout } from './callout';

export const Attribute = () => (
  <div>
    <AttributeIcons.AttributeStar />
    <Callout icons={AttributeIcons} />
  </div>
);
