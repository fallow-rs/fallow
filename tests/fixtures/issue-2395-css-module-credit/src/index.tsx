import { default as alias } from './alias.module.css';
import first from './first.module.css';
import second from './second.module.css';
import less from './theme.module.less';
import sass from './theme.module.sass';
import whole from './whole.module.css';

const hand = (classes: Record<string, string>): Record<string, string> => classes;

export const styles = [
  alias.aliasUsed,
  first.container,
  second.container,
  less.lessUsed,
  sass.sassUsed,
  whole.wholeRoot,
  hand(whole),
];
