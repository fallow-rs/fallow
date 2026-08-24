import { default as alias } from './alias.module.css';
import first from './first.module.css';
import second from './second.module.css';
import less from './theme.module.less';
import sass from './theme.module.sass';
import whole from './whole.module.css';
import extensionless from './extensionless.module';
import aliased from '@theme';
import extensionlessShadow from './shadow-extensionless.module';
import aliasedShadow from '@shadow-theme';

const hand = (classes: Record<string, string>): Record<string, string> => classes;
const previewExtensionlessShadow = (
  extensionlessShadow: Record<string, string>,
): string[] => [
  hand(extensionlessShadow).preview,
  extensionlessShadow.extensionlessShadowSpare,
];
const previewAliasedShadow = (aliasedShadow: Record<string, string>): string[] => [
  hand(aliasedShadow).preview,
  aliasedShadow.aliasedShadowSpare,
];

export const styles = [
  alias.aliasUsed,
  first.container,
  second.container,
  less.lessUsed,
  sass.sassUsed,
  whole.wholeRoot,
  hand(whole),
  extensionless.extensionlessUsed,
  hand(extensionless),
  aliased.aliasedUsed,
  hand(aliased),
  extensionlessShadow.extensionlessShadowUsed,
  aliasedShadow.aliasedShadowUsed,
  previewExtensionlessShadow,
  previewAliasedShadow,
];
