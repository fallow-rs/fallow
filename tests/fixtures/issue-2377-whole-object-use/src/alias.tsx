import * as AliasIcons from './alias-icons';

const alias = AliasIcons;

export const Aliased = () => (
  <div>
    <AliasIcons.AliasStar />
    {alias.AliasMoon()}
  </div>
);
