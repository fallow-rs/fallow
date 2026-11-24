import { ui, whole } from './styles';
import { ui as altUi } from './alt';
import { outer } from './theme';
import plain from './styles/plain';
import { settings } from './app-config';
import { settings as directSettings } from './config';

export { default as exposed } from './styles/exposed.module.css';

const hand = (classes: Record<string, string>): Record<string, string> => classes;

export const classNames = [
  ui.uiUsed,
  altUi.altUsed,
  outer.outerUsed,
  plain.plainUsed,
  hand(whole),
];
export const retries = [settings.retries, directSettings.retries];
